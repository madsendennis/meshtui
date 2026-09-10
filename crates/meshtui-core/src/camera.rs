use glam::{Mat3, Mat4, Quat, Vec3};

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

/// Orbital trackball camera. Orientation is a quaternion, so orbiting is free
/// tumbling with no pole clamping or gimbal lock. State is target +
/// orientation + distance; only this changes per interaction (scene geometry
/// is static).
#[derive(Debug, Clone)]
pub struct Camera {
    pub target: Vec3,
    /// Camera orientation: local +Z points from the target toward the camera,
    /// local +Y is screen up. `orbit` rotates this directly, so there are no
    /// angle clamps anywhere.
    pub orientation: Quat,
    pub distance: f32,
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
            // Default pose: camera on -Z of the target, looking along +Z, up +Y.
            orientation: look_rotation(Vec3::NEG_Z, Vec3::Y),
            distance,
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

    /// Unit vector from the target toward the camera.
    pub fn offset_dir(&self) -> Vec3 {
        self.orientation * Vec3::Z
    }

    /// Camera up axis in world space.
    pub fn up(&self) -> Vec3 {
        self.orientation * Vec3::Y
    }

    /// Camera right axis in world space.
    pub fn right(&self) -> Vec3 {
        self.orientation * Vec3::X
    }

    pub fn position(&self) -> Vec3 {
        self.target + self.offset_dir() * self.distance
    }

    pub fn view_matrix(&self) -> Mat4 {
        // Consistent eye/target order everywhere (fixes the fill-light
        // look-at order inconsistency from the Python version).
        // `up` is always perpendicular to the view direction (same rotation),
        // so look-at never degenerates.
        Mat4::look_at_rh(self.position(), self.target, self.up())
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

    /// Free trackball orbit: yaw around the camera's current up axis, pitch
    /// around its current right axis. No clamping — rotating past the poles
    /// just keeps tumbling.
    pub fn orbit(&mut self, delta_yaw: f32, delta_pitch: f32) {
        let yaw = Quat::from_axis_angle(self.up(), -delta_yaw);
        let pitch = Quat::from_axis_angle(self.right(), delta_pitch);
        self.orientation = (yaw * pitch * self.orientation).normalize();
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
        let dir = axis.direction().normalize();
        // Pick an up vector that is not parallel to the view direction.
        let up = if dir.dot(self.up()).abs() > 0.99 {
            if dir.dot(Vec3::Y).abs() > 0.99 {
                Vec3::Z
            } else {
                Vec3::Y
            }
        } else {
            self.up()
        };
        self.orientation = look_rotation(dir, up);
    }

    /// Set the orbit from spherical angles around `up` (compatibility for the
    /// initial_theta/initial_phi config keys). Same pose as the old spherical
    /// camera: phi is measured from +up, theta from the basis right vector.
    pub fn set_spherical(&mut self, theta: f32, phi: f32, up: Vec3) {
        let up = up.normalize_or(Vec3::Y);
        let phi = phi.clamp(MIN_PHI, std::f32::consts::PI - MIN_PHI);
        let (right, fwd) = orthonormal_basis(up);
        let dir =
            right * (phi.sin() * theta.cos()) + fwd * (phi.sin() * theta.sin()) + up * phi.cos();
        self.orientation = look_rotation(dir, up);
    }

    /// Roll the camera so its up axis matches `new_up`, keeping the view
    /// direction and position. No-op when looking along `new_up` (degenerate).
    pub fn set_up(&mut self, new_up: Vec3) {
        let new_up = new_up.normalize_or(Vec3::Y);
        let view_dir = -self.offset_dir();
        if view_dir.dot(new_up).abs() > 0.999 {
            return;
        }
        self.orientation = look_rotation(self.offset_dir(), new_up);
    }

    pub fn cycle_up(&mut self, up_vectors: &[[f32; 3]], forward: bool) {
        if up_vectors.is_empty() {
            return;
        }
        let idx = up_vectors
            .iter()
            .position(|v| {
                let v = Vec3::from(*v).normalize_or(Vec3::Y);
                v.dot(self.up()) > 0.999
            })
            .unwrap_or(0);
        let n = up_vectors.len();
        let next = if forward {
            (idx + 1) % n
        } else {
            (idx + n - 1) % n
        };
        self.set_up(Vec3::from(up_vectors[next]));
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

/// Orientation quaternion for a camera whose target→camera direction is
/// `back`, rolled so screen up matches `up` (projected onto the view plane).
fn look_rotation(back: Vec3, up: Vec3) -> Quat {
    let z = back.normalize_or(Vec3::Z);
    let f = -z;
    let s = f.cross(up);
    let s = if s.length_squared() < 1e-12 {
        // `up` parallel to the view direction: pick any perpendicular.
        f.cross(if f.y.abs() > 0.99 { Vec3::X } else { Vec3::Y })
    } else {
        s
    };
    let s = s.normalize_or(Vec3::X);
    let u = s.cross(f);
    Quat::from_mat3(&Mat3::from_cols(s, u, z)).normalize()
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
    fn orbit_tumbles_through_poles_without_locking() {
        let mut c = cam();
        let start = c.position();
        // A full pitch turn in small steps passes through both poles; the old
        // spherical camera clamped phi and never got there.
        for _ in 0..360 {
            c.orbit(0.0, std::f32::consts::PI / 180.0);
        }
        let end = c.position();
        assert!((start - end).length() < 1e-3, "start={start:?} end={end:?}");
        assert!((c.orientation.length() - 1.0).abs() < 1e-3);
    }

    #[test]
    fn orbit_past_pole_flips_camera_to_other_side() {
        let mut c = cam();
        let z0 = c.position().z;
        c.orbit(0.0, std::f32::consts::FRAC_PI_2); // to the pole
        c.orbit(0.0, std::f32::consts::FRAC_PI_2); // past it
        assert!(
            c.position().z * z0 < 0.0,
            "camera should end on the far side, z0={z0} z={}",
            c.position().z
        );
        // Screen up is now upside down in world space (continuous tumble).
        assert!(c.up().y < -0.999, "up={:?}", c.up());
    }

    #[test]
    fn orbit_yaw_full_circle_returns_to_start() {
        let mut c = cam();
        let start = c.position();
        for _ in 0..72 {
            c.orbit(std::f32::consts::TAU / 72.0, 0.0);
        }
        assert!((start - c.position()).length() < 1e-3);
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
        let mut c = cam(); // default up is +Y
        c.set_view_axis(ViewAxis::PosY);
        let dir = (c.position() - c.target).normalize();
        assert!(dir.dot(Vec3::Y) > 0.999, "dir={dir}");
        assert!(c.up().dot(Vec3::Y).abs() < 0.99, "up={:?}", c.up());
    }

    #[test]
    fn spherical_pose_matches_default_frame() {
        let mut c = cam();
        let default_pos = c.position();
        let default_up = c.up();
        c.set_spherical(0.0, std::f32::consts::FRAC_PI_2, Vec3::Y);
        assert!((c.position() - default_pos).length() < 1e-5);
        assert!((c.up() - default_up).length() < 1e-5);
    }

    #[test]
    fn set_up_rolls_camera_but_keeps_position() {
        let mut c = cam();
        let pos = c.position();
        c.set_up(Vec3::X);
        assert!((c.position() - pos).length() < 1e-5);
        assert!(c.up().dot(Vec3::X) > 0.999, "up={:?}", c.up());
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
        let orientation = c.orientation;
        let old_distance = c.distance;

        c.reframe_bounds(Vec3::ZERO, Vec3::splat(1.0), 1.0);

        assert!((c.target - Vec3::splat(0.5)).length() < 1e-5);
        assert_eq!(c.orientation, orientation);
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
