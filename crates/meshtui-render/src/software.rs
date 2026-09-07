//! Tile-parallel software rasterizer: perspective-correct triangle fill,
//! depth buffer, 3-point lighting, barycentric wireframe overlay.
//!
//! Strategy: the framebuffer is partitioned into 32×32 tiles. Each tile is
//! rasterized independently (in parallel with rayon) into local color/depth
//! buffers, then tiles are merged by depth. No locks, no atomics.

use glam::{Vec3, Vec4};
use rayon::prelude::*;

use meshtui_core::{Camera, CameraKind, Scene};

use crate::{Frame, RenderBackend};

const TILE: u32 = 32;

/// Lighting parameters (3-point system, intensities from config).
#[derive(Debug, Clone)]
pub struct Lighting {
    pub key_intensity: f32,
    pub fill_intensity: f32,
    pub fill_dir: Vec3,
    pub rim_intensity: f32,
    pub rim_dir: Vec3,
    pub ambient: [f32; 3],
}

/// Runtime render options.
#[derive(Debug, Clone)]
pub struct Options {
    pub lighting: Lighting,
    /// Blinn-Phong exponent, matching pygfx MeshPhongMaterial.
    pub shininess: f32,
    /// Wireframe overlay thickness in pixels; 0 disables.
    pub wireframe_thickness: f32,
    pub wireframe_color: [u8; 4],
}

/// The software fallback backend — a first-class citizen, not an afterthought.
pub struct SoftwareRasterizer {
    pub options: Options,
}

impl RenderBackend for SoftwareRasterizer {
    fn render(&mut self, scene: &Scene, camera: &Camera, width: u32, height: u32) -> Option<Frame> {
        if width == 0 || height == 0 {
            return None;
        }
        // Nothing visible → clean early-out (never NaN camera math upstream).
        let meshes: Vec<&meshtui_core::Mesh> = scene.visible_meshes().collect();
        if meshes.is_empty() {
            return None;
        }

        let view = camera.view_matrix();
        let proj = camera.proj_matrix(width, height);
        let vp = proj * view;
        let eye = camera.position();

        let camera_dir = (eye - camera.target).normalize_or(Vec3::Z);
        let orthographic = camera.kind == CameraKind::Orthographic;
        let w = width as usize;
        let _h = height as usize;
        let tiles_x = width.div_ceil(TILE);
        let tiles_y = height.div_ceil(TILE);
        let tile_count = (tiles_x as usize).checked_mul(tiles_y as usize)?;

        // 1. Clip and project all triangles ONCE into screen space (parallel
        //    over meshes), rejecting invalid and degenerate geometry.
        let hw = width as f32 * 0.5;
        let hh = height as f32 * 0.5;
        let screen_tris: Vec<ScreenTri> = meshes
            .par_iter()
            .flat_map_iter(|mesh| {
                let linear_color = [
                    srgb_to_linear(mesh.color[0]),
                    srgb_to_linear(mesh.color[1]),
                    srgb_to_linear(mesh.color[2]),
                    mesh.color[3],
                ];
                mesh.indices.as_chunks::<3>().0.iter().flat_map(move |tri| {
                    project_triangles(mesh, tri, vp, hw, hh, linear_color)
                        .into_iter()
                        .flatten()
                })
            })
            .collect();

        // 2. Bin triangles into the tiles their screen bbox overlaps.
        let mut bins: Vec<Vec<u32>> = (0..tile_count).map(|_| Vec::new()).collect();
        for (i, t) in screen_tris.iter().enumerate() {
            let min = t.s0.min(t.s1).min(t.s2);
            let max = t.s0.max(t.s1).max(t.s2);
            // Cull triangles fully outside the framebuffer — clamping alone
            // would wrongly bin them into the edge tiles.
            if max.x < 0.0 || max.y < 0.0 || min.x >= width as f32 || min.y >= height as f32 {
                continue;
            }
            let tx0 = ((min.x.floor() as i32).max(0) as u32 / TILE).min(tiles_x - 1);
            let ty0 = ((min.y.floor() as i32).max(0) as u32 / TILE).min(tiles_y - 1);
            let tx1 = ((max.x.ceil() as i32).max(0) as u32 / TILE).min(tiles_x - 1);
            let ty1 = ((max.y.ceil() as i32).max(0) as u32 / TILE).min(tiles_y - 1);
            for ty in ty0..=ty1 {
                for tx in tx0..=tx1 {
                    bins[(ty * tiles_x + tx) as usize].push(i as u32);
                }
            }
        }

        // 3. Per-tile color+depth buffers, rasterized in parallel; each
        //    tile only visits its own binned triangles.
        let tiles: Vec<(Vec<[u8; 4]>, Vec<f32>)> = bins
            .into_par_iter()
            .enumerate()
            .map(|(tile_idx, tris)| {
                let tx = tile_idx as u32 % tiles_x;
                let ty = tile_idx as u32 / tiles_x;
                let x0 = tx * TILE;
                let y0 = ty * TILE;
                let tw = (width - x0).min(TILE) as usize;
                let th = (height - y0).min(TILE) as usize;
                let mut color = vec![[0u8; 4]; tw * th];
                let mut depth = vec![f32::INFINITY; tw * th];
                for &i in &tris {
                    raster_triangle(
                        &screen_tris[i as usize],
                        eye,
                        camera_dir,
                        orthographic,
                        &self.options,
                        x0,
                        y0,
                        tw,
                        th,
                        &mut color,
                        &mut depth,
                    );
                }
                (color, depth)
            })
            .collect();

        // Merge tiles into the final frame (depth already resolved per-tile;
        // tiles are disjoint so a straight copy is enough).
        let mut frame = Frame::new(width, height)?;
        for (tile_idx, (color, _depth)) in tiles.iter().enumerate() {
            let tx = tile_idx as u32 % tiles_x;
            let ty = tile_idx as u32 / tiles_x;
            let x0 = tx * TILE;
            let y0 = ty * TILE;
            let tw = (width - x0).min(TILE) as usize;
            let th = (height - y0).min(TILE) as usize;
            for row in 0..th {
                let dst = ((y0 as usize + row) * w + x0 as usize) * 4;
                let src = row * tw;
                for col in 0..tw {
                    let c = color[src + col];
                    let d = dst + col * 4;
                    frame.pixels[d..d + 4].copy_from_slice(&c);
                }
            }
        }
        Some(frame)
    }
}

/// A triangle fully prepared for screen-space work: projected positions,
/// per-vertex reciprocal-w and NDC depth, world normals, and linear color.
struct ScreenTri {
    s0: glam::Vec2,
    s1: glam::Vec2,
    s2: glam::Vec2,
    z: [f32; 3],
    inv_w: [f32; 3],
    n: [Vec3; 3],
    world: [Vec3; 3],
    linear_color: [f32; 4],
    area: f32,
}

#[derive(Clone, Copy)]
struct ClipVertex {
    clip: Vec4,
    normal: Vec3,
    world: Vec3,
}

/// Clip one triangle to Glam's homogeneous 0..1 depth range, then project the
/// resulting polygon to screen-space triangles.
fn project_triangles(
    mesh: &meshtui_core::Mesh,
    tri: &[u32; 3],
    vp: glam::Mat4,
    hw: f32,
    hh: f32,
    linear_color: [f32; 4],
) -> [Option<ScreenTri>; 3] {
    let mut result = std::array::from_fn(|_| None);
    let make_vertex = |index: u32| -> Option<ClipVertex> {
        let idx = index as usize;
        let &p = mesh.positions.get(idx)?;
        if !p.is_finite() {
            return None;
        }
        let clip = vp * Vec4::new(p.x, p.y, p.z, 1.0);
        if !clip.is_finite() {
            return None;
        }
        Some(ClipVertex {
            clip,
            normal: mesh.normals.get(idx).copied().unwrap_or(Vec3::Z),
            world: p,
        })
    };
    let [Some(a), Some(b), Some(c)] = tri.map(make_vertex) else {
        return result;
    };
    let vertices = [a, b, c];

    // Most geometry is fully inside the depth range. Keep that common path
    // allocation-free; only triangles crossing near/far planes need clipping.
    if vertices
        .iter()
        .all(|vertex| vertex.clip.z >= 0.0 && vertex.clip.z <= vertex.clip.w)
    {
        result[0] = project_clipped_triangle(vertices, linear_color, hw, hh);
        return result;
    }

    let mut polygon = vertices.to_vec();
    for plane in 4..6 {
        polygon = clip_polygon(&polygon, plane);
        if polygon.len() < 3 {
            return result;
        }
    }

    for i in 1..polygon.len() - 1 {
        result[i - 1] = project_clipped_triangle(
            [polygon[0], polygon[i], polygon[i + 1]],
            linear_color,
            hw,
            hh,
        );
    }
    result
}

fn project_clipped_triangle(
    vertices: [ClipVertex; 3],
    linear_color: [f32; 4],
    hw: f32,
    hh: f32,
) -> Option<ScreenTri> {
    let project_vertex = |vertex: ClipVertex| {
        if vertex.clip.w <= f32::EPSILON {
            return None;
        }
        let inv_w = vertex.clip.w.recip();
        let ndc = vertex.clip.truncate() * inv_w;
        if !ndc.is_finite() {
            return None;
        }
        Some((
            glam::Vec2::new((ndc.x + 1.0) * hw, (1.0 - ndc.y) * hh),
            ndc.z.clamp(0.0, 1.0),
            inv_w,
            vertex.normal,
            vertex.world,
        ))
    };
    let [Some(a), Some(b), Some(c)] = vertices.map(project_vertex) else {
        return None;
    };
    let area = edge(a.0, b.0, c.0);
    (area.abs() >= 1e-7).then_some(ScreenTri {
        s0: a.0,
        s1: b.0,
        s2: c.0,
        z: [a.1, b.1, c.1],
        inv_w: [a.2, b.2, c.2],
        n: [a.3, b.3, c.3],
        world: [a.4, b.4, c.4],
        linear_color,
        area,
    })
}

fn clip_polygon(vertices: &[ClipVertex], plane: u8) -> Vec<ClipVertex> {
    let distance = |clip: Vec4| match plane {
        0 => clip.x + clip.w,
        1 => clip.w - clip.x,
        2 => clip.y + clip.w,
        3 => clip.w - clip.y,
        4 => clip.z,
        _ => clip.w - clip.z,
    };
    let mut output = Vec::with_capacity(vertices.len() + 1);
    let mut previous = vertices[vertices.len() - 1];
    let mut previous_distance = distance(previous.clip);
    for &current in vertices {
        let current_distance = distance(current.clip);
        let previous_inside = previous_distance >= 0.0;
        let current_inside = current_distance >= 0.0;
        if previous_inside != current_inside {
            let t = previous_distance / (previous_distance - current_distance);
            output.push(ClipVertex {
                clip: previous.clip.lerp(current.clip, t),
                normal: previous.normal.lerp(current.normal, t),
                world: previous.world.lerp(current.world, t),
            });
        }
        if current_inside {
            output.push(current);
        }
        previous = current;
        previous_distance = current_distance;
    }
    output
}

#[allow(clippy::too_many_arguments)]
fn raster_triangle(
    tri: &ScreenTri,
    eye: Vec3,
    camera_dir: Vec3,
    orthographic: bool,
    opts: &Options,
    tile_x: u32,
    tile_y: u32,
    tile_w: usize,
    tile_h: usize,
    out_color: &mut [[u8; 4]],
    out_depth: &mut [f32],
) {
    let (s0, s1, s2) = (tri.s0, tri.s1, tri.s2);

    // Screen-space bbox clipped to the tile. Compute in i32 and clamp to
    // the tile range on BOTH ends: a fully off-screen triangle must yield
    // an empty range, never a wrapped-around u32.
    let min = s0.min(s1).min(s2);
    let max = s0.max(s1).max(s2);
    let tile_max_x = (tile_x + tile_w as u32) as i32;
    let tile_max_y = (tile_y + tile_h as u32) as i32;
    let x_start = (min.x.floor() as i32).clamp(tile_x as i32, tile_max_x);
    let y_start = (min.y.floor() as i32).clamp(tile_y as i32, tile_max_y);
    let x_end = (max.x.ceil() as i32 + 1).clamp(tile_x as i32, tile_max_x);
    let y_end = (max.y.ceil() as i32 + 1).clamp(tile_y as i32, tile_max_y);
    if x_start >= x_end || y_start >= y_end {
        return;
    }

    let [inv_w0, inv_w1, inv_w2] = tri.inv_w;
    let [z0, z1, z2] = tri.z;

    for y in y_start..y_end {
        for x in x_start..x_end {
            let p = glam::Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            let w0 = edge(s1, s2, p) / tri.area;
            let w1 = edge(s2, s0, p) / tri.area;
            let w2 = edge(s0, s1, p) / tri.area;
            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                continue;
            }
            // NDC depth is already divided by clip-space w and therefore
            // interpolates linearly in screen space. Applying reciprocal-w
            // correction again breaks occlusion for slanted triangles.
            let inv_w = w0 * inv_w0 + w1 * inv_w1 + w2 * inv_w2;
            if inv_w <= 0.0 {
                continue;
            }
            let z = w0 * z0 + w1 * z1 + w2 * z2;
            let idx = (y as u32 - tile_y) as usize * tile_w + (x as u32 - tile_x) as usize;
            if z >= out_depth[idx] {
                continue;
            }

            // Perspective-correct world-space normal.
            let n =
                (tri.n[0] * (w0 * inv_w0) + tri.n[1] * (w1 * inv_w1) + tri.n[2] * (w2 * inv_w2))
                    / inv_w;
            let n = n.normalize_or(Vec3::Z);

            // Match a directional light parented to the camera: its direction
            // is exactly camera-local and cannot drift across the mesh as the
            // camera orbits. Only the perspective specular view vector varies
            // per fragment; orthographic view rays stay parallel.
            let view_dir = if orthographic {
                camera_dir
            } else {
                let world = (tri.world[0] * (w0 * inv_w0)
                    + tri.world[1] * (w1 * inv_w1)
                    + tri.world[2] * (w2 * inv_w2))
                    / inv_w;
                direction_to_eye(eye, world, camera_dir)
            };
            let px = shade(n, tri.linear_color, view_dir, camera_dir, opts);
            if opts.wireframe_thickness > 0.0 {
                // Barycentric-edge wireframe: distance to nearest edge.
                let d = wire_distance(s0, s1, s2, p);
                if d < opts.wireframe_thickness {
                    out_color[idx] = opts.wireframe_color;
                    out_depth[idx] = z;
                    continue;
                }
            }
            out_color[idx] = px;
            out_depth[idx] = z;
        }
    }
}

fn direction_to_eye(eye: Vec3, world: Vec3, fallback: Vec3) -> Vec3 {
    (eye - world).normalize_or(fallback)
}

/// Direction (surface → light) for a camera-anchored directional light,
/// offset from the camera direction by `azimuth_deg` (rotation around the
/// camera's up axis) and `elevation_deg` (tilt around the camera's right
/// axis).
///
/// Rotating in the camera-local frame keeps the offset meaningful from
/// every view; the naive global spherical offset degenerates when looking
/// along ±Z (azimuth collapses, so e.g. a 135° rim light lands right next
/// to the key instead of behind the object).
pub fn camera_light_offset(
    camera_dir: Vec3,
    camera_up: Vec3,
    azimuth_deg: f32,
    elevation_deg: f32,
) -> Vec3 {
    let fwd = camera_dir.normalize_or(Vec3::Z);
    let up = camera_up.normalize_or(Vec3::Y);
    // Reference up must not be parallel to the view direction.
    let up = if fwd.dot(up).abs() > 0.99 {
        if fwd.y.abs() < 0.9 {
            Vec3::Y
        } else {
            Vec3::X
        }
    } else {
        up
    };
    let right = fwd.cross(up).normalize_or(Vec3::X);
    let up = right.cross(fwd).normalize_or(Vec3::Y);
    let azimuth = glam::Mat3::from_axis_angle(up, azimuth_deg.to_radians());
    let elevation = glam::Mat3::from_axis_angle(right, elevation_deg.to_radians());
    (azimuth * elevation * fwd).normalize_or(fwd)
}

#[inline]
fn edge(a: glam::Vec2, b: glam::Vec2, p: glam::Vec2) -> f32 {
    (p.x - a.x) * (b.y - a.y) - (p.y - a.y) * (b.x - a.x)
}

/// True distance from p to the nearest triangle edge.
fn wire_distance(s0: glam::Vec2, s1: glam::Vec2, s2: glam::Vec2, p: glam::Vec2) -> f32 {
    fn seg_dist(a: glam::Vec2, b: glam::Vec2, p: glam::Vec2) -> f32 {
        let ab = b - a;
        let t = ((p - a).dot(ab) / ab.length_squared().max(1e-9)).clamp(0.0, 1.0);
        (p - (a + ab * t)).length()
    }
    seg_dist(s0, s1, p)
        .min(seg_dist(s1, s2, p))
        .min(seg_dist(s2, s0, p))
}

/// Lambert + Blinn-Phong specular per light, with ambient. Two-sided
/// (pygfx `side="both"` parity): the normal is flipped toward the viewer
/// first, then N·L is clamped per light — inward-wound geometry is lit by
/// the headlamp instead of going black. All light directions point from the
/// surface toward the light. The key direction is the camera's local forward
/// axis, matching a directional light parented to the camera.
fn shade(n: Vec3, linear_base: [f32; 4], view_dir: Vec3, key_dir: Vec3, opts: &Options) -> [u8; 4] {
    let l = &opts.lighting;
    let albedo = [linear_base[0], linear_base[1], linear_base[2]];
    let n = if n.dot(view_dir) < 0.0 { -n } else { n };
    // Ambient goes through the same Lambert BRDF as the diffuse terms
    // (pygfx BRDF_Lambert parity): irradiance * albedo / PI.
    let mut rgb = [
        albedo[0] * l.ambient[0] * std::f32::consts::FRAC_1_PI,
        albedo[1] * l.ambient[1] * std::f32::consts::FRAC_1_PI,
        albedo[2] * l.ambient[2] * std::f32::consts::FRAC_1_PI,
    ];
    for (dir, intensity) in [
        (key_dir, l.key_intensity),
        (l.fill_dir, l.fill_intensity),
        (l.rim_dir, l.rim_intensity),
    ] {
        if intensity <= 0.0 {
            continue;
        }
        // Single-sided lighting like pygfx: faces pointing away from a
        // light are not lit by it. The camera-anchored key light then
        // reads as a real headlamp (backfaces stay dark).
        let ndotl = n.dot(dir).max(0.0);
        let irradiance = ndotl * intensity;
        let diffuse = irradiance * std::f32::consts::FRAC_1_PI;
        let half = (dir + view_dir).normalize_or(dir);
        // Specular must disappear with diffuse incidence, as in pygfx's
        // RE_Direct, and the view vector must follow the camera.
        let spec = irradiance * n.dot(half).max(0.0).powf(opts.shininess.max(1.0)) * 0.12;
        rgb[0] += albedo[0] * diffuse + spec;
        rgb[1] += albedo[1] * diffuse + spec;
        rgb[2] += albedo[2] * diffuse + spec;
    }
    [
        (linear_to_srgb(rgb[0].clamp(0.0, 1.0)) * 255.0).round() as u8,
        (linear_to_srgb(rgb[1].clamp(0.0, 1.0)) * 255.0).round() as u8,
        (linear_to_srgb(rgb[2].clamp(0.0, 1.0)) * 255.0).round() as u8,
        (linear_base[3].clamp(0.0, 1.0) * 255.0) as u8,
    ]
}

fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(value: f32) -> f32 {
    if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use meshtui_core::{CameraKind, Mesh};

    fn opts() -> Options {
        Options {
            lighting: Lighting {
                key_intensity: 1.5,
                fill_intensity: 1.5,
                fill_dir: Vec3::new(-0.5, 0.3, -1.0).normalize(),
                rim_intensity: 0.6,
                rim_dir: Vec3::new(0.5, 0.2, 1.0).normalize(),
                ambient: [0.15, 0.15, 0.15],
            },
            shininess: 30.0,
            wireframe_thickness: 0.0,
            wireframe_color: [60, 60, 60, 255],
        }
    }

    fn quad_scene() -> Scene {
        let mut m = Mesh::new("quad");
        m.positions = vec![
            Vec3::new(-1.0, -1.0, 0.0),
            Vec3::new(1.0, -1.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(-1.0, 1.0, 0.0),
        ];
        m.normals = vec![Vec3::Z; 4];
        m.indices = vec![0, 1, 2, 0, 2, 3];
        m.color = [1.0, 0.0, 0.0, 1.0];
        let mut s = Scene::new();
        s.meshes.push(m);
        s
    }

    #[test]
    fn renders_center_pixel() {
        let scene = quad_scene();
        let (min, max) = scene.visible_bounds().unwrap();
        let mut cam = Camera::frame_bounds(min, max, CameraKind::Perspective, 60.0, 1.2);
        cam.set_view_axis(meshtui_core::ViewAxis::PosZ);
        let mut r = SoftwareRasterizer { options: opts() };
        let frame = r.render(&scene, &cam, 64, 64).expect("visible mesh");
        let center = (32 * 64 + 32) * 4;
        assert!(frame.pixels[center] > 200, "red center pixel");
        assert!(frame.pixels[center + 1] + 50 < frame.pixels[center]);
        assert_eq!(frame.pixels[center + 3], 255);
        // corner is background (transparent)
        assert_eq!(frame.pixels[3], 0);
    }

    #[test]
    fn hidden_scene_returns_none() {
        let mut scene = quad_scene();
        scene.meshes[0].visible = false;
        let (min, max) = (Vec3::splat(-1.0), Vec3::splat(1.0));
        let cam = Camera::frame_bounds(min, max, CameraKind::Perspective, 60.0, 1.0);
        let mut r = SoftwareRasterizer { options: opts() };
        assert!(r.render(&scene, &cam, 32, 32).is_none());
    }

    #[test]
    fn wireframe_overlay_draws_edges() {
        let scene = quad_scene();
        let (min, max) = scene.visible_bounds().unwrap();
        let mut cam = Camera::frame_bounds(min, max, CameraKind::Perspective, 60.0, 1.2);
        cam.set_view_axis(meshtui_core::ViewAxis::PosZ);
        let mut o = opts();
        o.wireframe_thickness = 1.5;
        let mut r = SoftwareRasterizer { options: o };
        let frame = r.render(&scene, &cam, 64, 64).unwrap();
        // some pixel must carry the wireframe color
        assert!(frame.pixels.as_chunks::<4>().0.contains(&[60, 60, 60, 255]));
    }

    #[test]
    fn offscreen_mesh_returns_promptly() {
        // Regression: off-screen triangles used to wrap the clipped bbox
        // through u32, looping ~4 billion pixels per triangle.
        let mut scene = quad_scene();
        for p in &mut scene.meshes[0].positions {
            p.x += 1000.0; // fully off the frustum
        }
        let cam = Camera::frame_bounds(
            Vec3::splat(-1.0),
            Vec3::splat(1.0),
            CameraKind::Perspective,
            60.0,
            1.0,
        );
        let mut r = SoftwareRasterizer { options: opts() };
        let frame = r.render(&scene, &cam, 64, 64).expect("still renders");
        assert!(frame.pixels.iter().all(|&b| b == 0));
    }

    #[test]
    fn extreme_zoom_returns_promptly() {
        // Regression: zooming far in left almost all triangles off-screen
        // and hung the rasterizer.
        let scene = quad_scene();
        let (min, max) = scene.visible_bounds().unwrap();
        let mut cam = Camera::frame_bounds(min, max, CameraKind::Perspective, 60.0, 1.2);
        cam.set_view_axis(meshtui_core::ViewAxis::PosZ);
        for _ in 0..200 {
            cam.zoom(0.9);
        }
        let mut r = SoftwareRasterizer { options: opts() };
        let start = std::time::Instant::now();
        let _ = r.render(&scene, &cam, 256, 256);
        assert!(start.elapsed().as_secs() < 2);
    }

    #[test]
    fn two_sided_shading_lights_inward_normals() {
        let mut options = opts();
        options.lighting.ambient = [0.0; 3];
        options.lighting.fill_intensity = 0.0;
        options.lighting.rim_intensity = 0.0;
        // A fragment whose normal points away from the camera (inward-wound
        // mesh) must still be lit by the headlamp, like pygfx side="both".
        let outward = shade(Vec3::Y, [0.5, 0.5, 0.5, 1.0], Vec3::Y, Vec3::Y, &options);
        let inward = shade(-Vec3::Y, [0.5, 0.5, 0.5, 1.0], Vec3::Y, Vec3::Y, &options);
        assert_eq!(outward, inward);
        assert!(outward[0] > 0);
    }

    #[test]
    fn camera_light_offset_keeps_rim_opposite_at_z_views() {
        // Regression: the global spherical offset collapsed azimuth when the
        // camera looked along ±Z, bunching the rim light next to the key.
        for fwd in [Vec3::Z, Vec3::NEG_Z, Vec3::X, Vec3::Y] {
            let rim = camera_light_offset(fwd, Vec3::Y, 135.0, 10.0);
            assert!(
                rim.dot(fwd) < -0.5,
                "rim must stay behind the object for fwd={fwd:?}, got {rim:?}"
            );
        }
    }

    #[test]
    fn camera_light_offset_handles_parallel_up() {
        let rim = camera_light_offset(Vec3::Y, Vec3::Y, 135.0, 10.0);
        assert!(rim.is_finite());
        assert!(rim.dot(Vec3::Y) < -0.5);
    }

    #[test]
    fn camera_light_rig_rotates_with_camera() {
        let forward = Vec3::Z;
        let up = Vec3::Y;
        let light = camera_light_offset(forward, up, -45.0, 15.0);
        let camera_rotation = glam::Mat3::from_axis_angle(Vec3::Y, 70.0_f32.to_radians())
            * glam::Mat3::from_axis_angle(Vec3::X, 25.0_f32.to_radians());
        let rotated_light =
            camera_light_offset(camera_rotation * forward, camera_rotation * up, -45.0, 15.0);
        assert!((rotated_light - camera_rotation * light).length() < 1e-5);
    }

    #[test]
    fn headlight_specular_follows_camera() {
        let mut options = opts();
        options.lighting.ambient = [0.0; 3];
        options.lighting.fill_intensity = 0.0;
        options.lighting.rim_intensity = 0.0;
        let toward_camera = shade(Vec3::Y, [0.0, 0.0, 0.0, 1.0], Vec3::Y, Vec3::Y, &options);
        let stale_world_z = shade(Vec3::Y, [0.0, 0.0, 0.0, 1.0], Vec3::Z, Vec3::Y, &options);
        assert!(toward_camera[0] > stale_world_z[0]);
    }

    #[test]
    fn perspective_view_direction_uses_fragment_to_camera_direction() {
        let eye = Vec3::new(0.0, 0.0, 5.0);
        let center = direction_to_eye(eye, Vec3::ZERO, Vec3::Z);
        let right = direction_to_eye(eye, Vec3::new(2.0, 0.0, 0.0), Vec3::Z);
        assert!(center.dot(Vec3::Z) > 0.999);
        assert!(right.x < 0.0);
        assert_ne!(center, right);
    }

    #[test]
    fn depth_uses_screen_space_interpolation() {
        let make_tri = |z, inv_w, color| ScreenTri {
            s0: glam::Vec2::new(0.0, 0.0),
            s1: glam::Vec2::new(3.0, 0.0),
            s2: glam::Vec2::new(0.0, 3.0),
            z,
            inv_w,
            n: [Vec3::Z; 3],
            world: [Vec3::ZERO; 3],
            linear_color: color,
            area: edge(
                glam::Vec2::new(0.0, 0.0),
                glam::Vec2::new(3.0, 0.0),
                glam::Vec2::new(0.0, 3.0),
            ),
        };
        let far_slanted = make_tri([0.1, 0.9, 0.9], [10.0, 1.0, 1.0], [1.0, 0.0, 0.0, 1.0]);
        let near_flat = make_tri([0.2; 3], [1.0; 3], [0.0, 0.0, 1.0, 1.0]);
        let mut options = opts();
        // PI * ambient == unlit full color through the Lambert BRDF.
        options.lighting.ambient = [std::f32::consts::PI; 3];
        options.lighting.key_intensity = 0.0;
        options.lighting.fill_intensity = 0.0;
        options.lighting.rim_intensity = 0.0;
        let mut color = vec![[0; 4]; 9];
        let mut depth = vec![f32::INFINITY; 9];
        for tri in [&far_slanted, &near_flat] {
            raster_triangle(
                tri,
                Vec3::Z,
                Vec3::Z,
                false,
                &options,
                0,
                0,
                3,
                3,
                &mut color,
                &mut depth,
            );
        }
        assert_eq!(color[0], [0, 0, 255, 255]);
    }

    #[test]
    fn triangle_crossing_near_plane_is_clipped() {
        let mut mesh = Mesh::new("near-plane");
        mesh.positions = vec![
            Vec3::new(-0.5, -0.5, -0.05),
            Vec3::new(0.5, -0.5, -1.0),
            Vec3::new(0.0, 0.5, -1.0),
        ];
        mesh.normals = vec![Vec3::Z; 3];
        mesh.indices = vec![0, 1, 2];
        let projection = glam::Mat4::perspective_rh(60.0f32.to_radians(), 1.0, 0.1, 10.0);
        let triangles = project_triangles(&mesh, &[0, 1, 2], projection, 32.0, 32.0, [1.0; 4]);
        assert!(triangles.iter().any(Option::is_some));
        assert!(triangles
            .iter()
            .flatten()
            .flat_map(|triangle| triangle.z)
            .all(|depth| (0.0..=1.0).contains(&depth)));
    }
}
