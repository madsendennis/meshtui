use glam::Vec3;
use rayon::prelude::*;

use crate::Color;

/// A single triangle mesh.
#[derive(Debug, Clone)]
pub struct Mesh {
    pub name: String,
    /// Flat xyz vertex positions, len = 3 * vertex_count.
    pub positions: Vec<Vec3>,
    /// Flat xyz per-vertex normals, len == positions.len().
    pub normals: Vec<Vec3>,
    /// Triangle index buffer, len = 3 * triangle_count.
    pub indices: Vec<u32>,
    /// Current display color (RGBA, linear).
    pub color: Color,
    /// Color authored in the file, if any (fixes "colors never used" bug).
    pub original_color: Option<Color>,
    pub visible: bool,
}

impl Mesh {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            positions: Vec::new(),
            normals: Vec::new(),
            indices: Vec::new(),
            color: [1.0, 1.0, 1.0, 1.0],
            original_color: None,
            visible: true,
        }
    }

    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty() || self.positions.is_empty()
    }

    /// Axis-aligned bounds, `None` for empty meshes (callers must handle this,
    /// unlike the Python version which crashed on empty meshes).
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        if self.positions.is_empty() || self.positions.iter().any(|p| !p.is_finite()) {
            return None;
        }
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for p in &self.positions {
            min = min.min(*p);
            max = max.max(*p);
        }
        Some((min, max))
    }

    /// Compute area-weighted smooth per-vertex normals. Face normals are
    /// computed in parallel; the vertex scatter is serial (cheap vs parsing).
    /// Call when the file provides no normals.
    pub fn compute_normals(&mut self) {
        let positions = &self.positions;
        let faces: Vec<Vec3> = self
            .indices
            .par_chunks_exact(3)
            .map(|tri| {
                let Some((&a, &b, &c)) = positions
                    .get(tri[0] as usize)
                    .zip(positions.get(tri[1] as usize))
                    .zip(positions.get(tri[2] as usize))
                    .map(|((a, b), c)| (a, b, c))
                else {
                    return Vec3::ZERO;
                };
                (b - a).cross(c - a)
            })
            .collect();
        let mut acc = vec![Vec3::ZERO; positions.len()];
        for (tri, face) in self.indices.as_chunks::<3>().0.iter().zip(faces) {
            for &i in tri {
                if let Some(normal) = acc.get_mut(i as usize) {
                    *normal += face;
                }
            }
        }
        self.normals = acc.into_iter().map(|n| n.normalize_or(Vec3::Z)).collect();
    }

    pub fn reset_color(&mut self) {
        self.color = self.original_color.unwrap_or([1.0, 1.0, 1.0, 1.0]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn triangle() -> Mesh {
        let mut m = Mesh::new("tri");
        m.positions = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        ];
        m.indices = vec![0, 1, 2];
        m
    }

    #[test]
    fn bounds_of_triangle() {
        let (min, max) = triangle().bounds().unwrap();
        assert_eq!(min, Vec3::ZERO);
        assert_eq!(max, Vec3::new(1.0, 1.0, 0.0));
    }

    #[test]
    fn empty_mesh_has_no_bounds() {
        assert!(Mesh::new("e").bounds().is_none());
    }

    #[test]
    fn non_finite_mesh_has_no_bounds() {
        let mut mesh = triangle();
        mesh.positions[0].x = f32::NAN;
        assert!(mesh.bounds().is_none());
    }

    #[test]
    fn computed_normal_points_up() {
        let mut m = triangle();
        m.compute_normals();
        for n in &m.normals {
            assert!((n.z - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn reset_color_uses_original() {
        let mut m = triangle();
        m.original_color = Some([1.0, 0.0, 0.0, 1.0]);
        m.color = [0.0, 1.0, 0.0, 1.0];
        m.reset_color();
        assert_eq!(m.color, [1.0, 0.0, 0.0, 1.0]);
    }
}
