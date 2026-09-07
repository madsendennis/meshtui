//! Core data model for meshtui: meshes, scene, camera, config, loading.

pub mod camera;
pub mod config;
pub mod loaders;
pub mod mesh;
pub mod scene;

pub use camera::{Camera, CameraKind, ViewAxis};
pub use mesh::Mesh;
pub use scene::Scene;

/// RGBA color, linear 0..=1 per channel.
pub type Color = [f32; 4];
