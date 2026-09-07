//! Kitty graphics protocol: escape-sequence generation and shared-memory
//! (`t=s`) transmission. No base64 for pixel data anywhere.

mod kitty;
pub mod theme;

pub use kitty::{
    delete_image, encode_png_fallback, image_channel, shm_transmit_escape, terminal_pixel_size,
    FileImage, ImageChannel, KittyError, ShmImage,
};
pub use theme::{resolve as resolve_theme, Theme, ThemeWatcher};
