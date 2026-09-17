//! Mesh statistics, MeshLab-style: counts, bounding box, area, volume.

use std::collections::HashSet;

use serde::Serialize;

use crate::{Color, Mesh};

/// Axis-aligned bounds plus derived measurements.
#[derive(Debug, Clone, Serialize)]
pub struct BoundsInfo {
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub size: [f32; 3],
    pub diagonal: f32,
    pub center: [f32; 3],
}

/// Statistics for one mesh. `None` bounds mean the mesh has no (finite)
/// vertices. `signed_volume` is only meaningful for closed meshes.
#[derive(Debug, Clone, Serialize)]
pub struct MeshInfo {
    pub name: String,
    pub vertices: usize,
    pub faces: usize,
    /// Unique undirected edges.
    pub edges: usize,
    pub bounds: Option<BoundsInfo>,
    pub surface_area: f64,
    pub signed_volume: f64,
    /// Color authored in the source file, if any.
    pub authored_color: Option<Color>,
}

impl MeshInfo {
    /// Compute statistics for a mesh.
    pub fn from_mesh(mesh: &Mesh) -> Self {
        let bounds = mesh.bounds().map(|(min, max)| {
            let size = max - min;
            BoundsInfo {
                min: min.to_array(),
                max: max.to_array(),
                size: size.to_array(),
                diagonal: size.length(),
                center: ((min + max) * 0.5).to_array(),
            }
        });
        let mut edges = HashSet::new();
        let mut surface_area = 0.0f64;
        let mut signed_volume = 0.0f64;
        for tri in mesh.indices.as_chunks::<3>().0 {
            let [a, b, c] = tri.map(|i| mesh.positions[i as usize]);
            for (u, v) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
                let (lo, hi) = (u64::from(u.min(v)), u64::from(u.max(v)));
                edges.insert(hi << 32 | lo);
            }
            surface_area += (b - a).cross(c - a).length() as f64 / 2.0;
            signed_volume += a.dot(b.cross(c)) as f64 / 6.0;
        }
        Self {
            name: mesh.name.clone(),
            vertices: mesh.vertex_count(),
            faces: mesh.triangle_count(),
            edges: edges.len(),
            bounds,
            surface_area,
            signed_volume,
            authored_color: mesh.original_color,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    fn unit_triangle() -> Mesh {
        let mut mesh = Mesh::new("tri");
        mesh.positions = vec![Vec3::ZERO, Vec3::X, Vec3::Y];
        mesh.indices = vec![0, 1, 2];
        mesh
    }

    #[test]
    fn triangle_stats() {
        let info = MeshInfo::from_mesh(&unit_triangle());
        assert_eq!(info.vertices, 3);
        assert_eq!(info.faces, 1);
        assert_eq!(info.edges, 3);
        assert!((info.surface_area - 0.5).abs() < 1e-6);
        assert!(
            info.signed_volume.abs() < 1e-6,
            "flat triangle has no volume"
        );
        let bounds = info.bounds.unwrap();
        assert_eq!(bounds.min, [0.0, 0.0, 0.0]);
        assert_eq!(bounds.size, [1.0, 1.0, 0.0]);
        assert!((bounds.diagonal - 2.0f32.sqrt()).abs() < 1e-5);
        assert_eq!(bounds.center, [0.5, 0.5, 0.0]);
    }

    #[test]
    fn tetrahedron_volume_and_shared_edges() {
        let mut mesh = Mesh::new("tet");
        mesh.positions = vec![Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z];
        mesh.indices = vec![0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3];
        let info = MeshInfo::from_mesh(&mesh);
        assert_eq!(info.faces, 4);
        assert_eq!(info.edges, 6, "shared edges counted once");
        assert!((info.signed_volume.abs() - 1.0 / 6.0).abs() < 1e-6);
    }

    #[test]
    fn empty_mesh_has_no_bounds() {
        let info = MeshInfo::from_mesh(&Mesh::new("empty"));
        assert!(info.bounds.is_none());
        assert_eq!(info.vertices, 0);
    }
}
