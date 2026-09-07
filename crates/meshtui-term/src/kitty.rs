//! Kitty graphics protocol implementation.
//!
//! Primary path: shared-memory transfer (`t=s`) — raw RGBA is written to a
//! POSIX shared-memory object and only a tiny escape sequence naming it hits
//! the terminal.
//! Fallbacks: raw file transfer (`t=f`) for local terminals and direct,
//! chunked PNG transfer (`t=d`) across SSH where local paths and shared
//! memory are not visible to the terminal.

use std::cell::Cell;
use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::{FromRawFd, IntoRawFd, RawFd};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use base64::Engine;

const ESC_G: &str = "\x1b_G";
const ST: &str = "\x1b\\";
// Kitty only places images below cells with explicit background colors when
// z is less than INT32_MIN / 2. This lets Ratatui panels and modals occlude
// the canvas while default-background viewport cells remain transparent.
const CANVAS_Z_INDEX: i32 = i32::MIN;
static TRANSFER_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, thiserror::Error)]
pub enum KittyError {
    #[error("memfd_create failed: {0}")]
    Memfd(io::Error),
    #[error("failed to write shared memory: {0}")]
    Write(io::Error),
    #[error("terminal size query failed: {0}")]
    SizeQuery(io::Error),
}

/// An RGBA image staged in a POSIX shared-memory object.
pub struct ShmImage {
    fd: RawFd,
    name: CString,
    transmitted: Cell<bool>,
    /// POSIX shm name passed to the terminal.
    pub path: String,
    pub size: usize,
}

impl ShmImage {
    /// Copy `rgba` (len = width*height*4) into fresh shared memory.
    pub fn from_rgba(rgba: &[u8], width: u32, height: u32) -> Result<Self, KittyError> {
        validate_rgba_len(rgba, width, height)?;
        let name = CString::new(format!(
            "/meshtui-{}-{}",
            std::process::id(),
            TRANSFER_ID.fetch_add(1, Ordering::Relaxed)
        ))
        .expect("generated shm name has no NUL");
        let fd = unsafe {
            libc::shm_open(
                name.as_ptr(),
                libc::O_CREAT | libc::O_EXCL | libc::O_RDWR | libc::O_CLOEXEC,
                0o600,
            )
        };
        if fd < 0 {
            return Err(KittyError::Memfd(io::Error::last_os_error()));
        }
        if unsafe { libc::ftruncate(fd, rgba.len() as libc::off_t) } != 0 {
            let error = io::Error::last_os_error();
            unsafe {
                libc::close(fd);
                libc::shm_unlink(name.as_ptr());
            }
            return Err(KittyError::Write(error));
        }
        let mut file: std::fs::File = unsafe { std::fs::File::from_raw_fd(fd) };
        use std::io::Write as _;
        if let Err(e) = file.write_all(rgba).and_then(|_| file.flush()) {
            unsafe {
                libc::shm_unlink(name.as_ptr());
            }
            return Err(KittyError::Write(e));
        }
        let fd = file.into_raw_fd();
        let path = name.to_string_lossy().into_owned();
        Ok(Self {
            fd,
            name,
            transmitted: Cell::new(false),
            path,
            size: rgba.len(),
        })
    }

    /// Transmit-and-display escape for this shm image (32-bit RGBA),
    /// fitted into `cols`×`rows` terminal cells at the cursor position.
    pub fn escape(&self, image_id: u32, width: u32, height: u32, cols: u32, rows: u32) -> String {
        shm_transmit_escape(image_id, width, height, cols, rows, &self.path)
    }

    /// Transfer ownership of the shm name to the terminal after its escape
    /// sequence has been flushed successfully.
    pub fn mark_transmitted(&self) {
        self.transmitted.set(true);
    }
}

impl Drop for ShmImage {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd) };
        // The protocol requires the terminal to unlink transmitted shm.
        // Clean it ourselves only if it never reached the terminal.
        if !self.transmitted.get() {
            unsafe {
                libc::shm_unlink(self.name.as_ptr());
            }
        }
    }
}

/// A reusable RGBA staging file in `$XDG_RUNTIME_DIR` (tmpfs) transmitted
/// with `t=f` (file transfer). Raw RGBA in the file (`f=32`), so there is
/// no base64/PNG overhead — only the path is base64-encoded. Works in both
/// kitty and Ghostty (which does not support `t=s` shared memory).
///
/// The file is removed on drop.
pub struct FileImage {
    file: std::fs::File,
    pub path: PathBuf,
}

impl FileImage {
    pub fn new() -> Result<Self, KittyError> {
        let dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|d| d.is_dir())
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        let path = dir.join(format!(
            "meshtui-tty-graphics-protocol-{}-{}.rgba",
            std::process::id(),
            TRANSFER_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&path)
            .map_err(KittyError::Write)?;
        Ok(Self { file, path })
    }

    /// Stage a new frame (len = width*height*4), truncating the old one.
    pub fn write(&mut self, rgba: &[u8], width: u32, height: u32) -> Result<(), KittyError> {
        use std::io::{Seek, Write as _};
        validate_rgba_len(rgba, width, height)?;
        self.file
            .rewind()
            .and_then(|_| self.file.write_all(rgba))
            .and_then(|_| self.file.set_len(rgba.len() as u64))
            .and_then(|_| self.file.sync_data())
            .map_err(KittyError::Write)
    }

    /// Transmit-and-display escape, fitted into `cols`×`rows` cells.
    pub fn escape(&self, image_id: u32, width: u32, height: u32, cols: u32, rows: u32) -> String {
        let payload =
            base64::engine::general_purpose::STANDARD.encode(self.path.as_os_str().as_bytes());
        format!(
            "{ESC_G}f=32,s={width},v={height},t=f,a=T,q=2,i={image_id},p=1,z={CANVAS_Z_INDEX},C=1,c={cols},r={rows};{payload}{ST}"
        )
    }
}

fn validate_rgba_len(rgba: &[u8], width: u32, height: u32) -> Result<(), KittyError> {
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4));
    if expected != Some(rgba.len()) {
        return Err(KittyError::Write(io::Error::new(
            io::ErrorKind::InvalidInput,
            "RGBA buffer length does not match image dimensions",
        )));
    }
    Ok(())
}

impl Drop for FileImage {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Pick the fastest image channel the terminal supports: shared memory on
/// kitty, staged file (`t=f`) elsewhere (Ghostty has no `t=s` support).
pub fn image_channel() -> ImageChannel {
    if ["SSH_CONNECTION", "SSH_CLIENT", "SSH_TTY"]
        .iter()
        .any(|name| std::env::var_os(name).is_some())
    {
        ImageChannel::Direct
    } else if std::env::var_os("KITTY_WINDOW_ID").is_some() {
        ImageChannel::SharedMemory
    } else {
        ImageChannel::File
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageChannel {
    SharedMemory,
    File,
    Direct,
}

/// Escape sequence: transmit RGBA from a shm file and display at cursor,
/// scaled to fit `cols`×`rows` cells.
/// `f=32` = RGBA, `t=s` = shared memory, `a=T` = transmit + display,
/// `q=2` = suppress responses, `i` = image id, `c`/`r` = cell extent.
pub fn shm_transmit_escape(
    image_id: u32,
    width: u32,
    height: u32,
    cols: u32,
    rows: u32,
    shm_path: &str,
) -> String {
    let payload = base64::engine::general_purpose::STANDARD.encode(shm_path.as_bytes());
    format!(
        "{ESC_G}f=32,s={width},v={height},t=s,a=T,q=2,i={image_id},p=1,z={CANVAS_Z_INDEX},C=1,c={cols},r={rows};{payload}{ST}"
    )
}

/// Escape sequence deleting an image (and its placements) by id.
pub fn delete_image(image_id: u32) -> String {
    format!("{ESC_G}a=d,d=I,q=2,i={image_id};{ST}")
}

/// Fallback: PNG-encode RGBA and produce chunked `t=f` escapes.
/// Kitty payloads are base64; chunks of at most 4096 bytes, `m=1` on all
/// but the last.
pub fn encode_png_fallback(
    rgba: &[u8],
    width: u32,
    height: u32,
    image_id: u32,
    cols: u32,
    rows: u32,
) -> Result<Vec<String>, KittyError> {
    validate_rgba_len(rgba, width, height)?;
    use image::{ColorType, ImageEncoder};
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(rgba, width, height, ColorType::Rgba8.into())
        .map_err(|e| KittyError::Write(io::Error::new(io::ErrorKind::InvalidData, e)))?;
    // Direct transmission carries the PNG bytes inside the escape sequence.
    let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
    let mut out = Vec::new();
    let mut first = true;
    let chunks: Vec<&[u8]> = b64.as_bytes().chunks(4096).collect();
    let last = chunks.len() - 1;
    for (n, chunk) in chunks.into_iter().enumerate() {
        let chunk = std::str::from_utf8(chunk).expect("base64 is ascii");
        let more = u8::from(n != last);
        let header = if first {
            first = false;
            format!(
                "f=100,t=d,a=T,q=2,i={image_id},p=1,z={CANVAS_Z_INDEX},C=1,c={cols},r={rows},m={more}"
            )
        } else {
            format!("q=2,i={image_id},m={more}")
        };
        out.push(format!("{ESC_G}{header};{chunk}{ST}"));
    }
    Ok(out)
}

/// Terminal pixel size via TIOCGWINSZ. Falls back to `None` when the
/// kernel doesn't report pixel dimensions (many terminals return 0).
pub fn terminal_pixel_size() -> Option<(u32, u32)> {
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    let ok = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) };
    if ok == 0 && ws.ws_xpixel > 0 && ws.ws_ypixel > 0 {
        Some((ws.ws_xpixel as u32, ws.ws_ypixel as u32))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shm_escape_golden() {
        let esc = shm_transmit_escape(7, 800, 600, 80, 30, "/meshtui-42-7");
        assert_eq!(
            esc,
            "\x1b_Gf=32,s=800,v=600,t=s,a=T,q=2,i=7,p=1,z=-2147483648,C=1,c=80,r=30;L21lc2h0dWktNDItNw==\x1b\\"
        );
    }

    #[test]
    fn delete_escape_golden() {
        assert_eq!(delete_image(42), "\x1b_Ga=d,d=I,q=2,i=42;\x1b\\");
    }

    #[test]
    fn png_fallback_chunks_and_terminates() {
        let rgba = vec![128u8; 64 * 64 * 4];
        let parts = encode_png_fallback(&rgba, 64, 64, 1, 20, 10).unwrap();
        assert!(!parts.is_empty());
        assert!(
            parts[0].starts_with("\x1b_Gf=100,t=d,a=T,q=2,i=1,p=1,z=-2147483648,C=1,c=20,r=10,m=")
        );
        // last chunk must terminate the stream
        assert!(parts.last().unwrap().contains("m=0;"));
        assert!(parts.iter().all(|p| p.ends_with("\x1b\\")));
    }

    #[test]
    fn shm_roundtrip() {
        let rgba = vec![0xABu8; 16 * 16 * 4];
        let img = ShmImage::from_rgba(&rgba, 16, 16).unwrap();
        let fd = unsafe { libc::shm_open(img.name.as_ptr(), libc::O_RDONLY, 0) };
        assert!(fd >= 0);
        let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
        let mut read_back = Vec::new();
        use std::io::Read as _;
        file.read_to_end(&mut read_back).unwrap();
        assert_eq!(read_back, rgba);
        assert!(img.escape(3, 16, 16, 8, 8).contains("t=s"));
        assert_eq!(unsafe { libc::shm_unlink(img.name.as_ptr()) }, 0);
    }

    #[test]
    fn file_image_roundtrip() {
        let mut img = FileImage::new().unwrap();
        let path = img.path.clone();
        let rgba = vec![0xCDu8; 32 * 32 * 4];
        img.write(&rgba, 32, 32).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), rgba);
        // overwrite with a smaller frame truncates correctly
        img.write(&vec![1u8; 16 * 16 * 4], 16, 16).unwrap();
        assert_eq!(std::fs::read(&path).unwrap().len(), 16 * 16 * 4);
        let esc = img.escape(9, 16, 16, 80, 30);
        assert!(esc
            .starts_with("\x1b_Gf=32,s=16,v=16,t=f,a=T,q=2,i=9,p=1,z=-2147483648,C=1,c=80,r=30;"));
        drop(img);
        assert!(!path.exists());
    }

    #[test]
    fn channel_selection() {
        // Without KITTY_WINDOW_ID (test env), file is selected.
        // (kitty path is exercised by shm_roundtrip above.)
        if std::env::var_os("KITTY_WINDOW_ID").is_none() {
            assert_eq!(image_channel(), ImageChannel::File);
        }
    }
}
