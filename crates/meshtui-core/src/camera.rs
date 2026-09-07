use glam::{Mat4, Vec3};

/// Preset view directions, matching the Python keybindings 1..6.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewAxis {
    PosX,
    NegX,
    PosY,
    NegY,
    PosZ,
    NegZ,
}

impl ViewAxis {
    pub fn direction(self) -> Vec3 {
        match self {
            ViewAxis::PosX => Vec3::X,
            ViewAxis::NegX => Vec3::NEG_X,
            ViewAxis::PosY => Vec3::Y,
            ViewAxis::NegY => Vec3::NEG_Y,
            ViewAxis::PosZ => Vec3::Z,
            ViewAxis::NegZ => Vec3::NEG_Z,
        }
    }

    /// Parse config strings like "+x", "-z".
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "+x" => Self::PosX,
            "-x" => Self::NegX,
            "+y" => Self::PosY,
            "-y" => Self::NegY,
            "+z" => Self::PosZ,
            "-z" => Self::NegZ,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraKind {
    Perspective,
    Orthographic,
}

/// Orbital camera. State is target + spherical angles + distance; only this
/// changes per interaction (scene geometry is static).
#[derive(Debug, Clone)]
pub struct Camera {
    pub target: Vec3,
    /// Horizontal angle (radians).
    pub theta: f32,
    /// Vertical angle from +up axis (radians), clamped away from the poles.
    pub phi: f32,
    pub distance: f32,
    pub up: Vec3,
    pub kind: CameraKind,
    /// Vertical FOV in degrees (perspective only).
    pub fov_degrees: f32,
    /// Scene radius used for orthographic framing and zoom clamping.
    pub scene_radius: f32,
    /// Orthographic projection scale. Kept separate from camera distance so
    /// zooming cannot move the eye through the mesh or break clipping.
    pub ortho_scale: f32,
}

const MIN_PHI: f32 = 1e-3;

impl Camera {
    /// Frame a scene given its bounds. Distance is padded so the whole
    /// bounding sphere fits (replicates `calculate_camera_parameters`).
    pub fn frame_bounds(
        bounds_min: Vec3,
        bounds_max: Vec3,
        kind: CameraKind,
        fov_degrees: f32,
        padding: f32,
    ) -> Self {
        let (center, radius, distance) = fit_bounds(bounds_min, bounds_max, fov_degrees, padding);
        Self {
            target: center,
            theta: 0.0,
            phi: std::f32::consts::FRAC_PI_2,
            distance,
            up: Vec3::Y,
            kind,
            fov_degrees,
            scene_radius: radius,
            ortho_scale: 1.0,
        }
    }

    /// Recenter and refit zoom to `bounds` while keeping the current orbit.
    ///
    /// Used when mesh visibility changes so a remaining mesh is framed instead
    /// of staying at the original scene zoom.
    pub fn reframe_bounds(&mut self, bounds_min: Vec3, bounds_max: Vec3, padding: f32) {
        let (center, radius, distance) =
            fit_bounds(bounds_min, bounds_max, self.fov_degrees, padding);
        self.target = center;
        self.scene_radius = radius;
        self.distance = distance;
        self.ortho_scale = 1.0;
    }

    pub fn position(&self) -> Vec3 {
        // Spherical coordinates around the target, measured against `up`.
        // Basis: build a frame from up.
        let (right, fwd) = orthonormal_basis(self.up);
        let dir = right * (self.phi.sin() * self.theta.cos())
            + fwd * (self.phi.sin() * self.theta.sin())
            + self.up * self.phi.cos();
        self.target + dir * self.distance
    }

    pub fn view_matrix(&self) -> Mat4 {
        // Consistent eye/target order everywhere (fixes the fill-light
        // look-at order inconsistency from the Python version).
        Mat4::look_at_rh(self.position(), self.target, self.up)
    }

    pub fn proj_matrix(&self, width: u32, height: u32) -> Mat4 {
        let aspect = (width.max(1) as f32) / (height.max(1) as f32);
        match self.kind {
            CameraKind::Perspective => Mat4::perspective_rh(
                self.fov_degrees.to_radians(),
                aspect,
                self.near(),
                self.far(),
            ),
            CameraKind::Orthographic => {
                let half_h =
                    self.distance * (self.fov_degrees.to_radians() * 0.5).tan() * self.ortho_scale;
                let half_w = half_h * aspect;
                Mat4::orthographic_rh(-half_w, half_w, -half_h, half_h, self.near(), self.far())
            }
        }
    }

    fn near(&self) -> f32 {
        (self.distance - self.scene_radius * 4.0).max(self.scene_radius * 1e-3)
    }

    fn far(&self) -> f32 {
        self.distance + self.scene_radius * 4.0
    }

    pub fn orbit(&mut self, delta_theta: f32, delta_phi: f32) {
        self.theta = (self.theta + delta_theta).rem_euclid(std::f32::consts::TAU);
        self.phi = (self.phi + delta_phi).clamp(MIN_PHI, std::f32::consts::PI - MIN_PHI);
    }

    pub fn zoom(&mut self, factor: f32) {
        match self.kind {
            CameraKind::Perspective => {
                self.distance = (self.distance * factor)
                    .clamp(self.scene_radius * 0.01, self.scene_radius * 100.0);
            }
            CameraKind::Orthographic => {
                self.ortho_scale = (self.ortho_scale * factor).clamp(0.01, 100.0);
            }
        }
    }

    /// Snap to a preset view axis: camera looks along -axis at the target.
    pub fn set_view_axis(&mut self, axis: ViewAxis) {
        let dir = axis.direction();
        // Pick an up vector that is not parallel to the view direction.
        self.up = if dir.dot(self.up).abs() > 0.99 {
            if dir.dot(Vec3::Y).abs() > 0.99 {
                Vec3::Z
            } else {
                Vec3::Y
            }
        } else {
            self.up
        };
        let (right, fwd) = orthonormal_basis(self.up);
        // Invert position(): dir = right*sin(phi)cos(theta) + fwd*sin(phi)sin(theta) + up*cos(phi)
        let d = dir.normalize();
        self.phi = d
            .dot(self.up)
            .clamp(-1.0, 1.0)
            .acos()
            .clamp(MIN_PHI, std::f32::consts::PI - MIN_PHI);
        let r = d.dot(right);
        let f = d.dot(fwd);
        self.theta = f.atan2(r);
    }

    pub fn cycle_up(&mut self, up_vectors: &[[f32; 3]], forward: bool) {
        if up_vectors.is_empty() {
            return;
        }
        let idx = up_vectors
            .iter()
            .position(|v| {
                let v = Vec3::from(*v).normalize_or(Vec3::Y);
                v.dot(self.up) > 0.999
            })
            .unwrap_or(0);
        let n = up_vectors.len();
        let next = if forward {
            (idx + 1) % n
        } else {
            (idx + n - 1) % n
        };
        let new_up = Vec3::from(up_vectors[next]).normalize_or(Vec3::Y);
        // Preserve the view direction as much as possible.
        let view_dir = (self.target - self.position()).normalize_or(Vec3::NEG_Z);
        if view_dir.dot(new_up).abs() > 0.999 {
            return; // would be degenerate
        }
        self.up = new_up;
        // Re-derive theta/phi against the new up.
        let cam_dir = -view_dir;
        let (right, fwd) = orthonormal_basis(self.up);
        self.phi = cam_dir
            .dot(self.up)
            .clamp(-1.0, 1.0)
            .acos()
            .clamp(MIN_PHI, std::f32::consts::PI - MIN_PHI);
        self.theta = cam_dir.dot(fwd).atan2(cam_dir.dot(right));
    }
}

fn fit_bounds(
    bounds_min: Vec3,
    bounds_max: Vec3,
    fov_degrees: f32,
    padding: f32,
) -> (Vec3, f32, f32) {
    let center = (bounds_min + bounds_max) * 0.5;
    let radius = ((bounds_max - bounds_min).length() * 0.5).max(1e-6);
    let fov = fov_degrees.to_radians();
    let distance = (radius / (fov * 0.5).sin()).max(radius) * padding;
    (center, radius, distance)
}

/// Right/forward basis perpendicular to `up`.
fn orthonormal_basis(up: Vec3) -> (Vec3, Vec3) {
    let up = up.normalize_or(Vec3::Y);
    let helper = if up.y.abs() > 0.99 { Vec3::X } else { Vec3::Y };
    let right = up.cross(helper).normalize_or(Vec3::X);
    let fwd = right.cross(up).normalize_or(Vec3::Z);
    (right, fwd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    fn cam() -> Camera {
        Camera::frame_bounds(
            Vec3::splat(-1.0),
            Vec3::splat(1.0),
            CameraKind::Perspective,
            60.0,
            1.0,
        )
    }

    #[test]
    fn position_distance_matches() {
        let c = cam();
        assert!((c.position().distance(c.target) - c.distance).abs() < 1e-4);
    }

    #[test]
    fn orbit_clamps_phi() {
        let mut c = cam();
        c.orbit(std::f32::consts::TAU * 100.0 + 0.5, 0.0);
        assert!((0.0..std::f32::consts::TAU).contains(&c.theta));
        c.orbit(0.0, 100.0);
        assert!(c.phi <= std::f32::consts::PI - MIN_PHI);
        c.orbit(0.0, -1000.0);
        assert!(c.phi >= MIN_PHI);
    }

    #[test]
    fn view_axis_snaps_camera() {
        let mut c = cam();
        c.set_view_axis(ViewAxis::PosZ);
        let dir = (c.position() - c.target).normalize();
        assert!(dir.dot(Vec3::Z) > 0.999, "dir={dir}");
    }

    #[test]
    fn view_axis_parallel_up_is_avoided() {
        let mut c = cam();
        c.up = Vec3::Y;
        c.set_view_axis(ViewAxis::PosY);
        assert!(c.up.dot(Vec3::Y).abs() < 0.99);
    }

    #[test]
    fn zoom_respects_clamps() {
        let mut c = cam();
        c.zoom(1e-9);
        assert!(c.distance >= c.scene_radius * 0.01);
        c.zoom(1e9);
        assert!(c.distance <= c.scene_radius * 100.0);
    }

    #[test]
    fn orthographic_zoom_does_not_move_camera() {
        let mut c = cam();
        c.kind = CameraKind::Orthographic;
        let position = c.position();
        c.zoom(0.5);
        assert_eq!(c.position(), position);
        assert_eq!(c.ortho_scale, 0.5);
    }

    #[test]
    fn view_axis_parse() {
        assert_eq!(ViewAxis::parse("+z"), Some(ViewAxis::PosZ));
        assert_eq!(ViewAxis::parse("bogus"), None);
    }

    #[test]
    fn reframe_fits_new_bounds_and_keeps_orbit() {
        let mut c = Camera::frame_bounds(
            Vec3::splat(-10.0),
            Vec3::splat(10.0),
            CameraKind::Perspective,
            60.0,
            1.0,
        );
        c.orbit(0.4, -0.2);
        let theta = c.theta;
        let phi = c.phi;
        let up = c.up;
        let old_distance = c.distance;

        c.reframe_bounds(Vec3::ZERO, Vec3::splat(1.0), 1.0);

        assert!((c.target - Vec3::splat(0.5)).length() < 1e-5);
        assert_eq!(c.theta, theta);
        assert_eq!(c.phi, phi);
        assert_eq!(c.up, up);
        assert_eq!(c.ortho_scale, 1.0);
        assert!(
            c.distance < old_distance * 0.5,
            "distance={:.3} old={:.3}",
            c.distance,
            old_distance
        );
        assert!(c.scene_radius < 1.0);
    }
}
