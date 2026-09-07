//! Rendering backends. The software rasterizer is the reference
//! implementation and fallback; a wgpu backend can slot in behind the same
//! `RenderBackend` trait later.

pub mod software;

use meshtui_core::{Camera, Scene};

/// RGBA8 framebuffer.
pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// Row-major RGBA, len = width * height * 4.
    pub pixels: Vec<u8>,
}

impl Frame {
    pub fn new(width: u32, height: u32) -> Option<Self> {
        let len = (width as usize)
            .checked_mul(height as usize)?
            .checked_mul(4)?;
        let mut pixels = Vec::new();
        pixels.try_reserve_exact(len).ok()?;
        pixels.resize(len, 0);
        Some(Self {
            width,
            height,
            pixels,
        })
    }
}

/// Backend-agnostic renderer interface (stage 2 contract).
pub trait RenderBackend {
    /// Render the visible meshes of `scene` with `camera` into a fresh frame.
    /// Returns `None` when there is nothing to draw (all meshes hidden) —
    /// callers must handle that instead of producing NaN-garbage.
    fn render(&mut self, scene: &Scene, camera: &Camera, width: u32, height: u32) -> Option<Frame>;
}
