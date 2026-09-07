use glam::Vec3;

use crate::mesh::Mesh;

/// The loaded scene: a flat list of meshes (no transforms, matching the
/// Python version's behavior).
#[derive(Debug, Default, Clone)]
pub struct Scene {
    pub meshes: Vec<Mesh>,
}

impl Scene {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn visible_meshes(&self) -> impl Iterator<Item = &Mesh> {
        self.meshes.iter().filter(|m| m.visible && !m.is_empty())
    }

    /// Combined bounds of all *visible, non-empty* meshes.
    /// Returns `None` when nothing is visible — callers must early-out
    /// (fixes the "hiding all meshes produces NaN render" bug).
    pub fn visible_bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        let mut any = false;
        for m in self.visible_meshes() {
            if let Some((bmin, bmax)) = m.bounds() {
                min = min.min(bmin);
                max = max.max(bmax);
                any = true;
            }
        }
        any.then_some((min, max))
    }

    pub fn total_triangles(&self) -> usize {
        self.visible_meshes().map(Mesh::triangle_count).sum()
    }

    pub fn total_vertices(&self) -> usize {
        self.visible_meshes().map(Mesh::vertex_count).sum()
    }

    pub fn visible_count(&self) -> usize {
        self.visible_meshes().count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_meshes_give_no_bounds() {
        let mut scene = Scene::new();
        let mut m = Mesh::new("a");
        m.positions = vec![Vec3::ZERO, Vec3::X, Vec3::Y];
        m.indices = vec![0, 1, 2];
        m.visible = false;
        scene.meshes.push(m);
        assert!(scene.visible_bounds().is_none());
    }
}
