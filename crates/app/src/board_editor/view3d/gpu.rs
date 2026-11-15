//! The 3D view drawn on the GPU (wgpu, through egui's paint callback). The scene goes into an off-screen picture at `SS` times the window's
//! resolution (colour, the part at each pixel, depth); a last pass inside egui's own drawing shrinks it into the window and draws the
//! outlines of the part under the pointer and of the selected part from the part image, as the software picture does (`raster3d.rs`,
//! which stays: for saving a PNG, the tests, and when there is no GPU).
//!
//! The camera is the software one (`Cam`): the same view-space rotation, and the clip coordinates written so that the GPU's perspective
//! divide gives exactly `Cam::project`'s screen point. Lighting is `scene::light` on the face's view-space normal; the board's faces are
//! the board pictures, unlit. The part under the pointer is found on the CPU (`pick`): the pointer's ray against the parts' triangles.
//!
//! Looks (`Look`): the colours as the software picture, or white ("clay", every face a light grey except the parts over the case height and
//! the highlighted net). Ambient occlusion (`ao`): the scene pass also writes each pixel's normal; an AO pass looks round every window pixel
//! within `AO_RADIUS` mm (16 directions over the hemisphere of its normal, turned by a hash of the pixel) and darkens it as much as other
//! surfaces stand in front of those points (screen-space AO from the depth image); the last pass blurs it over 5 × 5 pixels of about the same
//! depth (so it does not bleed over edges) and multiplies the colour by it.

use super::*;
use eframe::egui_wgpu;
use eframe::wgpu;
use std::sync::Arc;

/// The off-screen picture is this many times the window's pixels in each direction (smooth edges after shrinking). The AO shader takes 2 as
/// written (`fs_ao`).
const SS: u32 = 2;
const BACKGROUND: [u8; 3] = [34, 38, 46];

/// Set once at start-up when the window is drawn with wgpu (`init`): the 3D view may then use the GPU.
static READY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) fn ready() -> bool {
    READY.load(std::sync::atomic::Ordering::Relaxed)
}

/// One vertex of a face: position (mm), view-independent normal, colour, part (`NO_PART` for the board), picture coordinates and which
/// picture (0 none, 1 top, 2 bottom).
#[derive(Clone, Copy)]
struct Vtx {
    pos: [f32; 3],
    normal: [f32; 3],
    color: [u8; 4],
    part: u32,
    uv: [f32; 2],
    tex: u32,
}
const VTX_BYTES: u64 = 44;

fn push_vtx(out: &mut Vec<u8>, v: &Vtx) {
    for f in v.pos.iter().chain(&v.normal) {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out.extend_from_slice(&v.color);
    out.extend_from_slice(&v.part.to_le_bytes());
    for f in &v.uv {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out.extend_from_slice(&v.tex.to_le_bytes());
}

/// A line's end: position and colour; `alpha` 254 marks a part's face edge (grey in the white look), 255 a case line.
fn push_line_vtx(out: &mut Vec<u8>, p: [f32; 3], c: [u8; 3], alpha: u8) {
    for f in &p {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out.extend_from_slice(&[c[0], c[1], c[2], alpha]);
}
const LINE_BYTES: u64 = 16;

/// The scene as vertex data for the GPU, made on the CPU when the scene or what shows in it changes (`key`).
pub(super) struct Geometry {
    key: (u64, u32),
    tris: Vec<u8>,
    /// Vertices of the board alone (the parts follow).
    board_verts: u32,
    verts: u32,
    lines: Vec<u8>,
    board_lines: u32,
    line_verts: u32,
    /// The board pictures (top, bottom): width, height, RGBA.
    pics: [(u32, u32, Arc<Vec<u8>>); 2],
}

/// Every scene gets its own number, so the GPU's copy is replaced when another scene is built.
static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub(super) fn next_serial() -> u64 {
    SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

fn f3(p: [f64; 3]) -> [f32; 3] {
    p.map(|v| v as f32)
}

fn face_normal(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> Option<[f32; 3]> {
    let (u, w) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
    let n = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    (len > 1e-12).then(|| [(n[0] / len) as f32, (n[1] / len) as f32, (n[2] / len) as f32])
}

impl Geometry {
    /// The scene and generation it was made for.
    pub(super) fn key(&self) -> (u64, u32) {
        self.key
    }

    /// Vertices of the board alone and of everything (three a triangle).
    #[cfg(test)]
    pub(super) fn counts(&self) -> (u32, u32) {
        (self.board_verts, self.verts)
    }

    pub(super) fn build(scene: &Scene, generation: u32) -> Geometry {
        let mut tris = vec![];
        let tri = |out: &mut Vec<u8>, vs: [Vtx; 3]| {
            for v in &vs {
                push_vtx(out, v);
            }
        };
        let count = |bytes: &Vec<u8>, each: u64| (bytes.len() as u64 / each) as u32;
        let fr = scene.pv.frame;
        let flat = |p: [f64; 3], normal: [f32; 3], color: [u8; 3], part: u32| Vtx { pos: f3(p), normal, color: [color[0], color[1], color[2], 255], part, uv: [0.0; 2], tex: 0 };
        // the board's faces, painted with the board pictures (mapped by the picture's own frame, as the software picture does)
        for bottom in [false, true] {
            let z = if bottom { 0.0 } else { BOARD_THICKNESS };
            let normal = [0.0, 0.0, if bottom { -1.0 } else { 1.0 }];
            for t in &scene.tris {
                let corner = |p: &Point| {
                    let u = ((p.x - fr.x0) * fr.px / fr.w as f64) as f32;
                    let v = ((fr.y1 - p.y) * fr.px / fr.h as f64) as f32;
                    Vtx { pos: f3([p.x, p.y, z]), normal, color: [255; 4], part: NO_PART, uv: [if bottom { 1.0 - u } else { u }, v], tex: 1 + bottom as u32 }
                };
                tri(&mut tris, [corner(&t[0]), corner(&t[1]), corner(&t[2])]);
            }
            let zg = if bottom { -GLOW_LIFT } else { BOARD_THICKNESS + GLOW_LIFT };
            for (_, t) in scene.glow.iter().filter(|(b, _)| *b == bottom) {
                let vs = t.map(|p| Vtx { pos: f3([p.x, p.y, zg]), normal, color: [GLOW[0], GLOW[1], GLOW[2], 255], part: NO_PART, uv: [0.0; 2], tex: 3 });
                tri(&mut tris, vs);
            }
        }
        // the edge of the board
        let k = scene.outline.len();
        for i in 0..k {
            let (a, b) = (scene.outline[i], scene.outline[(i + 1) % k]);
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            let len = dx.hypot(dy);
            if len < 1e-9 {
                continue;
            }
            let normal = [(dy / len) as f32, (-dx / len) as f32, 0.0];
            let c = |x: f64, y: f64, z: f64| flat([x, y, z], normal, [176, 150, 92], NO_PART);
            let (p0, p1, p2, p3) = (c(a.x, a.y, 0.0), c(b.x, b.y, 0.0), c(b.x, b.y, BOARD_THICKNESS), c(a.x, a.y, BOARD_THICKNESS));
            tri(&mut tris, [p0, p1, p2]);
            tri(&mut tris, [p0, p2, p3]);
        }
        let board_verts = count(&tris, VTX_BYTES);
        // the lines: the case height rings and the case box
        let mut lines = vec![];
        let line = |out: &mut Vec<u8>, a: [f64; 3], b: [f64; 3], c: [u8; 3]| {
            push_line_vtx(out, f3(a), c, 255);
            push_line_vtx(out, f3(b), c, 255);
        };
        let edge = |out: &mut Vec<u8>, a: [f64; 3], b: [f64; 3], c: [u8; 3]| {
            push_line_vtx(out, f3(a), c, 254);
            push_line_vtx(out, f3(b), c, 254);
        };
        for (side, ring) in scene.rings.iter().enumerate() {
            if let Some(l) = ring {
                let z = if side == 0 { BOARD_THICKNESS + l } else { -l };
                for i in 0..k {
                    let (a, b) = (scene.outline[i], scene.outline[(i + 1) % k]);
                    line(&mut lines, [a.x, a.y, z], [b.x, b.y, z], [255, 170, 40]);
                }
            }
        }
        if let Some(w) = &scene.case {
            let col = [90, 170, 255];
            let corners = [(w.lo.x, w.lo.y), (w.hi.x, w.lo.y), (w.hi.x, w.hi.y), (w.lo.x, w.hi.y)];
            for i in 0..4 {
                let (a, b) = (corners[i], corners[(i + 1) % 4]);
                for z in [w.z0, w.z1] {
                    line(&mut lines, [a.0, a.1, z], [b.0, b.1, z], col);
                }
                line(&mut lines, [a.0, a.1, w.z0], [a.0, a.1, w.z1], col);
            }
        }
        let board_lines = count(&lines, LINE_BYTES);
        // the parts: guessed solids (with a thin dark edge round every face) and makers' meshes; a part over the case height is red
        let red = [225, 50, 40];
        let warn = |mut v: Vtx, on: bool| {
            if on {
                v.tex = 4;
            }
            v
        };
        for b in &scene.boxes {
            let too_tall = scene.over.get(b.part).is_some_and(|o| *o > 1e-6);
            let color = if too_tall { red } else { b.color };
            for (corners, normal) in solid_faces(b) {
                for i in 1..corners.len().saturating_sub(1) {
                    let v = |p: [f64; 3]| warn(flat(p, normal, color, b.part as u32), too_tall);
                    tri(&mut tris, [v(corners[0]), v(corners[i]), v(corners[i + 1])]);
                }
                let dark = shade3(b.color, 0.4);
                for i in 0..corners.len() {
                    edge(&mut lines, corners[i], corners[(i + 1) % corners.len()], dark);
                }
            }
        }
        for m in &scene.meshes {
            let too_tall = scene.over.get(m.part).is_some_and(|o| *o > 1e-6);
            let v: Vec<[f64; 3]> = m.mesh.vertices.iter().map(|p| p.map(f64::from)).collect();
            for (t, c) in m.mesh.triangles.iter().zip(&m.mesh.tri_color) {
                let (a, b, d) = (v[t[0] as usize], v[t[1] as usize], v[t[2] as usize]);
                let Some(normal) = face_normal(a, b, d) else { continue };
                let base = m.mesh.palette.get(*c as usize).copied().unwrap_or([140, 140, 140]);
                let color = if too_tall { red } else { base };
                let vv = |p: [f64; 3]| warn(flat(p, normal, color, m.part as u32), too_tall);
                tri(&mut tris, [vv(a), vv(b), vv(d)]);
            }
        }
        let pic = |i: usize| (scene.pv.frame.w as u32, scene.pv.frame.h as u32, Arc::new(scene.img[i].iter().flatten().copied().collect::<Vec<u8>>()));
        let (verts, line_verts) = (count(&tris, VTX_BYTES), count(&lines, LINE_BYTES));
        Geometry { key: (scene.serial, generation), tris, board_verts, verts, lines, board_lines, line_verts, pics: [pic(0), pic(1)] }
    }
}

/// How the faces are painted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(super) enum Look {
    /// The board pictures and the parts' colours, as the software picture.
    #[default]
    Color,
    /// Every face white (light grey), to see the shapes; with ambient occlusion it reads like a clay model.
    Clay,
}

/// What one frame of the window asks the GPU for.
pub(super) struct Frame {
    pub geometry: Arc<Geometry>,
    pub cam: Cam,
    /// The window's rectangle (points) and pixels per point.
    pub rect: Rect,
    pub ppp: f32,
    pub show_parts: bool,
    pub hover: Option<usize>,
    pub selected: Option<usize>,
    pub look: Look,
    /// Darken the creases and corners (ambient occlusion).
    pub ao: bool,
}

/// How far (mm) round each point the ambient occlusion looks for surfaces in front.
const AO_RADIUS: f32 = 2.0;

/// The camera as the shader takes it: the rotation rows, the centre and distance, how view space maps to clip space, the depth range and
/// the parts to mark.
fn camera_uniform(f: &Frame) -> Vec<u8> {
    let c = &f.cam;
    let (sy, cy) = c.yaw.sin_cos();
    let (sp, cp) = c.pitch.sin_cos();
    // `Cam::dir` as rows: x' = (cy, -sy, 0), up = (sy sp, cy sp, cp), w = (-sy cp, -cy cp, sp)
    let rows = [[cy, -sy, 0.0, 0.0], [sy * sp, cy * sp, cp, 0.0], [-sy * cp, -cy * cp, sp, 0.0]];
    let (w, h) = (f.rect.width().max(1.0), f.rect.height().max(1.0));
    // screen = origin + v * scale * dist / W  →  ndc = a + v * s / W
    let proj = [c.scale * c.dist * 2.0 / w, c.scale * c.dist * 2.0 / h, (c.origin.x - f.rect.min.x) * 2.0 / w - 1.0, 1.0 - (c.origin.y - f.rect.min.y) * 2.0 / h];
    let near = c.dist * 0.05;
    let far = c.dist * 4.0;
    let mut out = vec![];
    for r in rows {
        for v in r {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    for v in [c.center[0], c.center[1], c.center[2], c.dist] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for v in proj {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for v in [near, far] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for v in [f.hover.map_or(NO_PART, |h| h as u32), f.selected.map_or(NO_PART, |s| s as u32), (f.look == Look::Clay) as u32, f.ao as u32] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for v in [AO_RADIUS, 0.0, 0.0, 0.0] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}
const CAMERA_BYTES: u64 = 3 * 16 + 16 + 16 + 16 + 16 + 16;

const SHADER: &str = r#"
struct Camera {
    r0: vec4<f32>, r1: vec4<f32>, r2: vec4<f32>,
    center_dist: vec4<f32>,
    proj: vec4<f32>,
    near: f32, far: f32, hover: u32, selected: u32,
    clay: u32, ao: u32, ao_radius: f32, _pad: f32,
};
@group(0) @binding(0) var<uniform> cam: Camera;
@group(1) @binding(0) var top_pic: texture_2d<f32>;
@group(1) @binding(1) var bottom_pic: texture_2d<f32>;
@group(1) @binding(2) var pic_sampler: sampler;

fn to_view(p: vec3<f32>) -> vec3<f32> {
    let rel = p - cam.center_dist.xyz;
    return vec3<f32>(dot(cam.r0.xyz, rel), dot(cam.r1.xyz, rel), dot(cam.r2.xyz, rel));
}

// `lift` (mm) pulls the depth towards the viewer without moving the point on the screen (lines over the faces they lie on)
fn to_clip_lifted(v: vec3<f32>, lift: f32) -> vec4<f32> {
    let w = cam.center_dist.w - v.z;
    // depth after the divide: 0 at `near`, 1 at `far`, in 1/w (which the GPU interpolates correctly across a triangle)
    let wd = w - lift;
    let z = (cam.far * wd - cam.near * cam.far) / (cam.far - cam.near) * (w / wd);
    return vec4<f32>(v.x * cam.proj.x + cam.proj.z * w, v.y * cam.proj.y + cam.proj.w * w, z, w);
}

fn to_clip(v: vec3<f32>) -> vec4<f32> {
    return to_clip_lifted(v, 0.0);
}

struct FaceOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) @interpolate(flat) part: u32,
    @location(4) @interpolate(flat) tex: u32,
};

@vertex
fn vs_face(@location(0) pos: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) color: vec4<f32>, @location(3) part: u32, @location(4) uv: vec2<f32>, @location(5) tex: u32) -> FaceOut {
    var o: FaceOut;
    o.pos = to_clip(to_view(pos));
    o.color = color;
    o.normal = vec3<f32>(dot(cam.r0.xyz, normal), dot(cam.r1.xyz, normal), dot(cam.r2.xyz, normal));
    o.uv = uv;
    o.part = part;
    o.tex = tex;
    return o;
}

struct Targets {
    @location(0) color: vec4<f32>,
    @location(1) part: u32,
    @location(2) normal: vec4<f32>,
};

@fragment
fn fs_face(i: FaceOut) -> Targets {
    var o: Targets;
    o.part = i.part;
    let n = normalize(i.normal);
    if (n.z <= 1e-4) { discard; }
    o.normal = vec4<f32>(n * 0.5 + vec3<f32>(0.5), 1.0);
    // white: the board and the parts light grey, lit; the highlighted net and the parts over the case height keep their colours
    var base = i.color.rgb;
    var lit = true;
    if (cam.clay == 1u) {
        if (i.tex == 1u || i.tex == 2u) { base = vec3<f32>(0.80); }
        else if (i.tex == 0u) { base = vec3<f32>(0.93); }
        if (i.tex == 3u) { lit = false; }
    } else if (i.tex == 1u) {
        o.color = vec4<f32>(textureSample(top_pic, pic_sampler, i.uv).rgb, 1.0);
        return o;
    } else if (i.tex == 2u) {
        o.color = vec4<f32>(textureSample(bottom_pic, pic_sampler, i.uv).rgb, 1.0);
        return o;
    } else if (i.tex == 3u) {
        lit = false;
    }
    if (!lit) {
        o.color = vec4<f32>(base, 1.0);
        return o;
    }
    // lit from above-left of the viewer, as scene::light; a face turned away from the viewer is not drawn (as the software picture)
    let l = vec3<f32>(-0.35, 0.45, 0.82);
    let k = clamp(0.45 + 0.6 * max(dot(n, l), 0.0), 0.0, 1.3);
    var c = min(base * k, vec3<f32>(1.0));
    if (i.part == cam.hover && cam.hover != 0xffffffffu) {
        c = min(c + vec3<f32>(50.0, 50.0, 30.0) / 255.0, vec3<f32>(1.0));
    }
    o.color = vec4<f32>(c, 1.0);
    return o;
}

struct LineOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_line(@location(0) pos: vec3<f32>, @location(1) color: vec4<f32>) -> LineOut {
    var o: LineOut;
    // 0.03 mm nearer than the faces they lie on, so they are not hidden by them (a fixed step in the depth scale would be millimetres at a
    // distance and let the edges behind a part show through)
    o.pos = to_clip_lifted(to_view(pos), 0.03);
    o.color = color;
    return o;
}

@fragment
fn fs_line(i: LineOut) -> Targets {
    var o: Targets;
    o.color = vec4<f32>(i.color.rgb, 1.0);
    // white: the parts' face edges (alpha below 1) are left out (the shading and the occlusion show the shape); the case lines stay
    if (cam.clay == 1u && i.color.a < 0.999) { discard; }
    o.part = 0xffffffffu;
    o.normal = vec4<f32>(0.5, 0.5, 1.0, 1.0);
    return o;
}
"#;

const AO: &str = r#"
struct Camera {
    r0: vec4<f32>, r1: vec4<f32>, r2: vec4<f32>,
    center_dist: vec4<f32>,
    proj: vec4<f32>,
    near: f32, far: f32, hover: u32, selected: u32,
    clay: u32, ao: u32, ao_radius: f32, _pad: f32,
};
@group(0) @binding(0) var<uniform> cam: Camera;
@group(1) @binding(0) var depth_img: texture_depth_2d;
@group(1) @binding(1) var normal_img: texture_2d<f32>;

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32((i << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(i & 2u) * 2.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

// the view-space point of a texel of the scene pictures, from its depth
fn view_at(t: vec2<i32>, size: vec2<f32>) -> vec4<f32> {
    let d = textureLoad(depth_img, t, 0);
    if (d >= 1.0) { return vec4<f32>(0.0, 0.0, 0.0, 0.0); }
    let w = cam.near * cam.far / (cam.far - d * (cam.far - cam.near));
    let ndc = vec2<f32>((f32(t.x) + 0.5) / size.x * 2.0 - 1.0, 1.0 - (f32(t.y) + 0.5) / size.y * 2.0);
    return vec4<f32>((ndc.x - cam.proj.z) * w / cam.proj.x, (ndc.y - cam.proj.w) * w / cam.proj.y, cam.center_dist.w - w, 1.0);
}

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

@fragment
fn fs_ao(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(depth_img));
    let out_px = vec2<i32>(floor(frag.xy));
    // the scene pictures are SS (2) times the window: the middle of this pixel's block of texels
    let ss = 2;
    let t = out_px * ss + vec2<i32>(ss / 2);
    let p = view_at(t, size);
    if (p.w == 0.0) { return vec4<f32>(1.0); }
    let n = normalize(textureLoad(normal_img, t, 0).xyz * 2.0 - vec3<f32>(1.0));
    // a frame round the normal, turned by a hash of the pixel so the samples differ from pixel to pixel (the blur evens them out)
    let a = hash(frag.xy) * 6.2831853;
    var tangent = normalize(cross(n, select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), abs(n.x) > 0.8)));
    let b0 = cross(n, tangent);
    tangent = cos(a) * tangent + sin(a) * b0;
    let bitangent = cross(n, tangent);
    var occluded = 0.0;
    let count = 16;
    for (var k = 0; k < count; k++) {
        // a spiral over the hemisphere, nearer the point first
        let fk = f32(k) + 0.5;
        let up = 1.0 - fk / f32(count);
        let r = sqrt(1.0 - up * up);
        let phi = fk * 2.39996323;
        let dir = tangent * (r * cos(phi)) + bitangent * (r * sin(phi)) + n * max(up, 0.15);
        let scale = mix(0.25, 1.0, (fk / f32(count)) * (fk / f32(count)));
        let s = p.xyz + dir * cam.ao_radius * scale;
        let w = cam.center_dist.w - s.z;
        if (w <= cam.near) { continue; }
        let ndc = vec2<f32>(s.x * cam.proj.x / w + cam.proj.z, s.y * cam.proj.y / w + cam.proj.w);
        let tt = vec2<i32>(floor(vec2<f32>((ndc.x + 1.0) * 0.5 * size.x, (1.0 - ndc.y) * 0.5 * size.y)));
        if (tt.x < 0 || tt.y < 0 || tt.x >= i32(size.x) || tt.y >= i32(size.y)) { continue; }
        let q = view_at(tt, size);
        if (q.w == 0.0) { continue; }
        // a surface in front of the sample point, near enough to count
        let range = smoothstep(0.0, 1.0, cam.ao_radius / max(abs(p.z - q.z), 1e-4));
        if (q.z >= s.z + 0.02) { occluded += range; }
    }
    let ao = clamp(1.0 - occluded / f32(count) * 1.8, 0.0, 1.0);
    return vec4<f32>(ao, ao, ao, 1.0);
}
"#;

const COMPOSITE: &str = r#"
struct Comp { ss: u32, hover: u32, selected: u32, ao: u32, origin: vec2<f32>, near: f32, far: f32 };
@group(0) @binding(0) var color_img: texture_2d<f32>;
@group(0) @binding(1) var part_img: texture_2d<u32>;
@group(0) @binding(2) var<uniform> comp: Comp;
@group(0) @binding(3) var ao_img: texture_2d<f32>;
@group(0) @binding(4) var depth_img: texture_depth_2d;

// the distance from the eye at a window pixel (the middle of its block of scene texels); 0 for the background
fn eye_dist(px: vec2<i32>) -> f32 {
    let ss = i32(comp.ss);
    let size = vec2<i32>(textureDimensions(depth_img));
    let t = clamp(px * ss + vec2<i32>(ss / 2), vec2<i32>(0), size - vec2<i32>(1));
    let d = textureLoad(depth_img, t, 0);
    if (d >= 1.0) { return 0.0; }
    return comp.near * comp.far / (comp.far - d * (comp.far - comp.near));
}

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32((i << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(i & 2u) * 2.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn part_at(p: vec2<i32>) -> u32 {
    let size = vec2<i32>(textureDimensions(part_img));
    let q = clamp(p, vec2<i32>(0), size - vec2<i32>(1));
    return textureLoad(part_img, q, 0).x;
}

@fragment
fn fs_full(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let px = vec2<i32>(floor(frag.xy - comp.origin));
    let ss = i32(comp.ss);
    var sum = vec3<f32>(0.0);
    for (var y = 0; y < ss; y++) {
        for (var x = 0; x < ss; x++) {
            sum += textureLoad(color_img, px * ss + vec2<i32>(x, y), 0).rgb;
        }
    }
    var c = sum / f32(ss * ss);
    if (comp.ao == 1u) {
        // the occlusion blurred over 5 x 5 pixels of about the same distance from the eye (so it does not bleed over an edge)
        let w0 = eye_dist(px);
        if (w0 > 0.0) {
            let size = vec2<i32>(textureDimensions(ao_img));
            var total = 0.0;
            var weight = 0.0;
            for (var dy = -2; dy <= 2; dy++) {
                for (var dx = -2; dx <= 2; dx++) {
                    let q = clamp(px + vec2<i32>(dx, dy), vec2<i32>(0), size - vec2<i32>(1));
                    let w = eye_dist(q);
                    if (w > 0.0 && abs(w - w0) < 0.01 * w0) {
                        total += textureLoad(ao_img, q, 0).r;
                        weight += 1.0;
                    }
                }
            }
            if (weight > 0.0) { c = c * (total / weight); }
        }
    }
    // an outline just outside the selected part (cyan) and the part under the pointer (yellow), two pixels wide
    let centre = px * ss + vec2<i32>(ss / 2);
    let own = part_at(centre);
    var near_sel = false;
    var near_hov = false;
    for (var dy = -2; dy <= 2; dy++) {
        for (var dx = -2; dx <= 2; dx++) {
            let other = part_at(centre + vec2<i32>(dx, dy) * ss);
            if (other != own && other != 0xffffffffu) {
                if (other == comp.selected) { near_sel = true; }
                if (other == comp.hover) { near_hov = true; }
            }
        }
    }
    if (near_hov && own != comp.hover) { c = vec3<f32>(1.0, 230.0 / 255.0, 90.0 / 255.0); }
    else if (near_sel && own != comp.selected) { c = vec3<f32>(90.0 / 255.0, 220.0 / 255.0, 1.0); }
    return vec4<f32>(c, 1.0);
}
"#;

/// Off-screen pictures of one size (the scene's at `SS` times the window, the occlusion's at the window's).
struct Targets {
    size: [u32; 2],
    depth: wgpu::TextureView,
    color: wgpu::TextureView,
    part: wgpu::TextureView,
    normal: wgpu::TextureView,
    ao: wgpu::TextureView,
    ao_bind: wgpu::BindGroup,
    comp_bind: wgpu::BindGroup,
}

/// The GPU's copy of a scene.
struct Uploaded {
    key: (u64, u32),
    tris: wgpu::Buffer,
    board_verts: u32,
    verts: u32,
    lines: Option<wgpu::Buffer>,
    board_lines: u32,
    line_verts: u32,
    pics: wgpu::BindGroup,
}

/// Everything the GPU keeps between frames (in egui-wgpu's callback resources).
pub(crate) struct Resources {
    face_pipe: wgpu::RenderPipeline,
    line_pipe: wgpu::RenderPipeline,
    ao_pipe: wgpu::RenderPipeline,
    comp_pipe: wgpu::RenderPipeline,
    pic_layout: wgpu::BindGroupLayout,
    ao_layout: wgpu::BindGroupLayout,
    comp_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    cam_buf: wgpu::Buffer,
    cam_bind: wgpu::BindGroup,
    comp_buf: wgpu::Buffer,
    targets: Option<Targets>,
    scene: Option<Uploaded>,
}

const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const PART_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Uint;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const NORMAL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const AO_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

/// Makes the pipelines and hands them to egui-wgpu; called once when the window is made with wgpu. `msaa` is the sample count of egui's
/// own drawing (the last pass draws into it).
pub(crate) fn init(rs: &egui_wgpu::RenderState, msaa: u32) {
    let device = &rs.device;
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("view3d"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
    let comp_module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("view3d_composite"), source: wgpu::ShaderSource::Wgsl(COMPOSITE.into()) });
    let ao_module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("view3d_ao"), source: wgpu::ShaderSource::Wgsl(AO.into()) });
    let uniform_entry = |binding: u32, vis: wgpu::ShaderStages| wgpu::BindGroupLayoutEntry { binding, visibility: vis, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None };
    let cam_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("view3d_cam"), entries: &[uniform_entry(0, wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT)] });
    let tex_entry = |binding: u32, sample: wgpu::TextureSampleType| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture { sample_type: sample, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
        count: None,
    };
    let pic_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("view3d_pics"),
        entries: &[
            tex_entry(0, wgpu::TextureSampleType::Float { filterable: true }),
            tex_entry(1, wgpu::TextureSampleType::Float { filterable: true }),
            wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
        ],
    });
    let comp_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("view3d_comp"),
        entries: &[
            tex_entry(0, wgpu::TextureSampleType::Float { filterable: false }),
            tex_entry(1, wgpu::TextureSampleType::Uint),
            uniform_entry(2, wgpu::ShaderStages::FRAGMENT),
            tex_entry(3, wgpu::TextureSampleType::Float { filterable: false }),
            tex_entry(4, wgpu::TextureSampleType::Depth),
        ],
    });
    let ao_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("view3d_ao"),
        entries: &[tex_entry(0, wgpu::TextureSampleType::Depth), tex_entry(1, wgpu::TextureSampleType::Float { filterable: false })],
    });
    let scene_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("view3d_scene"), bind_group_layouts: &[Some(&cam_layout), Some(&pic_layout)], immediate_size: 0 });
    let comp_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("view3d_comp"), bind_group_layouts: &[Some(&comp_layout)], immediate_size: 0 });
    let ao_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("view3d_ao"), bind_group_layouts: &[Some(&cam_layout), Some(&ao_layout)], immediate_size: 0 });
    let targets = [
        Some(wgpu::ColorTargetState { format: COLOR_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL }),
        Some(wgpu::ColorTargetState { format: PART_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL }),
        Some(wgpu::ColorTargetState { format: NORMAL_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL }),
    ];
    let depth = Some(wgpu::DepthStencilState { format: DEPTH_FORMAT, depth_write_enabled: Some(true), depth_compare: Some(wgpu::CompareFunction::LessEqual), stencil: wgpu::StencilState::default(), bias: wgpu::DepthBiasState::default() });
    let scene_pipe = |label: &str, vs: &str, fs: &str, layout: wgpu::VertexBufferLayout, topology: wgpu::PrimitiveTopology| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&scene_layout),
            vertex: wgpu::VertexState { module: &module, entry_point: Some(vs), buffers: &[Some(layout)], compilation_options: Default::default() },
            primitive: wgpu::PrimitiveState { topology, ..Default::default() },
            depth_stencil: depth.clone(),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some(fs), targets: &targets, compilation_options: Default::default() }),
            multiview_mask: None,
            cache: None,
        })
    };
    let face_pipe = scene_pipe(
        "view3d_faces",
        "vs_face",
        "fs_face",
        wgpu::VertexBufferLayout { array_stride: VTX_BYTES, step_mode: wgpu::VertexStepMode::Vertex, attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Unorm8x4, 3 => Uint32, 4 => Float32x2, 5 => Uint32] },
        wgpu::PrimitiveTopology::TriangleList,
    );
    let line_pipe = scene_pipe(
        "view3d_lines",
        "vs_line",
        "fs_line",
        wgpu::VertexBufferLayout { array_stride: LINE_BYTES, step_mode: wgpu::VertexStepMode::Vertex, attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Unorm8x4] },
        wgpu::PrimitiveTopology::LineList,
    );
    let ao_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("view3d_ao"),
        layout: Some(&ao_pl),
        vertex: wgpu::VertexState { module: &ao_module, entry_point: Some("vs_full"), buffers: &[], compilation_options: Default::default() },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState { module: &ao_module, entry_point: Some("fs_ao"), targets: &[Some(wgpu::ColorTargetState { format: AO_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })], compilation_options: Default::default() }),
        multiview_mask: None,
        cache: None,
    });
    let comp_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("view3d_composite"),
        layout: Some(&comp_pl),
        vertex: wgpu::VertexState { module: &comp_module, entry_point: Some("vs_full"), buffers: &[], compilation_options: Default::default() },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState { count: msaa.max(1), mask: !0, alpha_to_coverage_enabled: false },
        fragment: Some(wgpu::FragmentState { module: &comp_module, entry_point: Some("fs_full"), targets: &[Some(wgpu::ColorTargetState { format: rs.target_format, blend: None, write_mask: wgpu::ColorWrites::ALL })], compilation_options: Default::default() }),
        multiview_mask: None,
        cache: None,
    });
    let cam_buf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("view3d_cam"), size: CAMERA_BYTES, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
    let cam_bind = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("view3d_cam"), layout: &cam_layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: cam_buf.as_entire_binding() }] });
    let comp_buf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("view3d_comp"), size: 32, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor { label: Some("view3d_pics"), mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
    rs.renderer.write().callback_resources.insert(Resources { face_pipe, line_pipe, ao_pipe, comp_pipe, pic_layout, ao_layout, comp_layout, sampler, cam_buf, cam_bind, comp_buf, targets: None, scene: None });
    READY.store(true, std::sync::atomic::Ordering::Relaxed);
}

fn buffer_with(device: &wgpu::Device, queue: &wgpu::Queue, label: &str, data: &[u8]) -> wgpu::Buffer {
    let buf = device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: data.len().max(4) as u64, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
    queue.write_buffer(&buf, 0, data);
    buf
}

fn picture(device: &wgpu::Device, queue: &wgpu::Queue, (w, h, rgba): &(u32, u32, Arc<Vec<u8>>)) -> wgpu::TextureView {
    let size = wgpu::Extent3d { width: (*w).max(1), height: (*h).max(1), depth_or_array_layers: 1 };
    let tex = device.create_texture(&wgpu::TextureDescriptor { label: Some("view3d_pic"), size, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8Unorm, usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[] });
    if rgba.len() as u32 == w * h * 4 && *w > 0 && *h > 0 {
        queue.write_texture(tex.as_image_copy(), rgba, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(*h) }, size);
    }
    tex.create_view(&Default::default())
}

fn target(device: &wgpu::Device, size: [u32; 2], format: wgpu::TextureFormat, binding: bool) -> wgpu::TextureView {
    let usage = if binding { wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING } else { wgpu::TextureUsages::RENDER_ATTACHMENT };
    device
        .create_texture(&wgpu::TextureDescriptor { label: Some("view3d_target"), size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format, usage, view_formats: &[] })
        .create_view(&Default::default())
}

/// The paint callback of one frame.
pub(super) struct Paint(pub Frame);

impl egui_wgpu::CallbackTrait for Paint {
    fn prepare(&self, device: &wgpu::Device, queue: &wgpu::Queue, _sd: &egui_wgpu::ScreenDescriptor, encoder: &mut wgpu::CommandEncoder, res: &mut egui_wgpu::CallbackResources) -> Vec<wgpu::CommandBuffer> {
        let Some(r) = res.get_mut::<Resources>() else { return vec![] };
        let f = &self.0;
        let g = &f.geometry;
        if r.scene.as_ref().is_none_or(|s| s.key != g.key) {
            let (top, bottom) = (picture(device, queue, &g.pics[0]), picture(device, queue, &g.pics[1]));
            let pics = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("view3d_pics"),
                layout: &r.pic_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&top) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&bottom) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&r.sampler) },
                ],
            });
            r.scene = Some(Uploaded {
                key: g.key,
                tris: buffer_with(device, queue, "view3d_tris", &g.tris),
                board_verts: g.board_verts,
                verts: g.verts,
                lines: (!g.lines.is_empty()).then(|| buffer_with(device, queue, "view3d_lines", &g.lines)),
                board_lines: g.board_lines,
                line_verts: g.line_verts,
                pics,
            });
        }
        let size = [((f.rect.width() * f.ppp).round().max(1.0) as u32) * SS, ((f.rect.height() * f.ppp).round().max(1.0) as u32) * SS];
        if r.targets.as_ref().is_none_or(|t| t.size != size) {
            let (depth, color, part, normal) = (target(device, size, DEPTH_FORMAT, true), target(device, size, COLOR_FORMAT, true), target(device, size, PART_FORMAT, true), target(device, size, NORMAL_FORMAT, true));
            let ao = target(device, [size[0] / SS, size[1] / SS], AO_FORMAT, true);
            let ao_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("view3d_ao"),
                layout: &r.ao_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&depth) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&normal) }],
            });
            let comp_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("view3d_comp"),
                layout: &r.comp_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&color) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&part) },
                    wgpu::BindGroupEntry { binding: 2, resource: r.comp_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&ao) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&depth) },
                ],
            });
            r.targets = Some(Targets { size, depth, color, part, normal, ao, ao_bind, comp_bind });
        }
        queue.write_buffer(&r.cam_buf, 0, &camera_uniform(f));
        let mut comp = vec![];
        for v in [SS, f.hover.map_or(NO_PART, |h| h as u32), f.selected.map_or(NO_PART, |s| s as u32), f.ao as u32] {
            comp.extend_from_slice(&v.to_le_bytes());
        }
        for v in [(f.rect.min.x * f.ppp).round(), (f.rect.min.y * f.ppp).round(), f.cam.dist * 0.05, f.cam.dist * 4.0] {
            comp.extend_from_slice(&v.to_le_bytes());
        }
        queue.write_buffer(&r.comp_buf, 0, &comp);
        let (Some(t), Some(s)) = (&r.targets, &r.scene) else { return vec![] };
        let bg = BACKGROUND.map(|c| f64::from(c) / 255.0);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("view3d_scene"),
            color_attachments: &[
                Some(wgpu::RenderPassColorAttachment { view: &t.color, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: bg[0], g: bg[1], b: bg[2], a: 1.0 }), store: wgpu::StoreOp::Store }, depth_slice: None }),
                Some(wgpu::RenderPassColorAttachment { view: &t.part, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: f64::from(NO_PART), g: 0.0, b: 0.0, a: 0.0 }), store: wgpu::StoreOp::Store }, depth_slice: None }),
                Some(wgpu::RenderPassColorAttachment { view: &t.normal, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.5, g: 0.5, b: 1.0, a: 1.0 }), store: wgpu::StoreOp::Store }, depth_slice: None }),
            ],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &t.depth, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }), stencil_ops: None }),
            occlusion_query_set: None,
            timestamp_writes: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &r.cam_bind, &[]);
        pass.set_bind_group(1, &s.pics, &[]);
        pass.set_pipeline(&r.face_pipe);
        pass.set_vertex_buffer(0, s.tris.slice(..));
        pass.draw(0..if f.show_parts { s.verts } else { s.board_verts }, 0..1);
        if let Some(lines) = &s.lines {
            pass.set_pipeline(&r.line_pipe);
            pass.set_vertex_buffer(0, lines.slice(..));
            pass.draw(0..if f.show_parts { s.line_verts } else { s.board_lines }, 0..1);
        }
        drop(pass);
        if f.ao {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("view3d_ao"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &t.ao, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::WHITE), store: wgpu::StoreOp::Store }, depth_slice: None })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&r.ao_pipe);
            pass.set_bind_group(0, &r.cam_bind, &[]);
            pass.set_bind_group(1, &t.ao_bind, &[]);
            pass.draw(0..3, 0..1);
        }
        vec![]
    }

    fn paint(&self, _info: egui::PaintCallbackInfo, pass: &mut wgpu::RenderPass<'static>, res: &egui_wgpu::CallbackResources) {
        let Some(r) = res.get::<Resources>() else { return };
        let Some(t) = &r.targets else { return };
        pass.set_pipeline(&r.comp_pipe);
        pass.set_bind_group(0, &t.comp_bind, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// The part under the pointer `p` (window points): the nearest of the parts' triangles the pointer's ray meets, unless the board is in
/// front of it.
pub(super) fn pick(scene: &Scene, cam: &Cam, p: Pos2, show_parts: bool) -> Option<usize> {
    if !show_parts {
        return None;
    }
    // the ray, in view space: from the eye (0, 0, dist) through the point on the plane w = 0 that projects to `p`
    let (x0, y0) = ((p.x - cam.origin.x) / cam.scale, (cam.origin.y - p.y) / cam.scale);
    let (sy, cy) = cam.yaw.sin_cos();
    let (sp, cp) = cam.pitch.sin_cos();
    // back to world space: the transpose of `Cam::dir`'s rotation
    let rows = [[cy, -sy, 0.0], [sy * sp, cy * sp, cp], [-sy * cp, -cy * cp, sp]];
    let to_world = |v: [f32; 3]| -> [f64; 3] { [0, 1, 2].map(|k| f64::from(rows[0][k] * v[0] + rows[1][k] * v[1] + rows[2][k] * v[2])) };
    let c = cam.center.map(f64::from);
    let e = to_world([0.0, 0.0, cam.dist]);
    let origin = [e[0] + c[0], e[1] + c[1], e[2] + c[2]];
    let d = to_world([x0, y0, -cam.dist]);
    let hit_tri = |a: [f64; 3], b: [f64; 3], q: [f64; 3]| -> Option<f64> {
        // Möller–Trumbore
        let (e1, e2) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [q[0] - a[0], q[1] - a[1], q[2] - a[2]]);
        let pv = [d[1] * e2[2] - d[2] * e2[1], d[2] * e2[0] - d[0] * e2[2], d[0] * e2[1] - d[1] * e2[0]];
        let det = e1[0] * pv[0] + e1[1] * pv[1] + e1[2] * pv[2];
        if det.abs() < 1e-12 {
            return None;
        }
        let tv = [origin[0] - a[0], origin[1] - a[1], origin[2] - a[2]];
        let u = (tv[0] * pv[0] + tv[1] * pv[1] + tv[2] * pv[2]) / det;
        if !(0.0..=1.0).contains(&u) {
            return None;
        }
        let qv = [tv[1] * e1[2] - tv[2] * e1[1], tv[2] * e1[0] - tv[0] * e1[2], tv[0] * e1[1] - tv[1] * e1[0]];
        let v = (d[0] * qv[0] + d[1] * qv[1] + d[2] * qv[2]) / det;
        if v < 0.0 || u + v > 1.0 {
            return None;
        }
        let t = (e2[0] * qv[0] + e2[1] * qv[1] + e2[2] * qv[2]) / det;
        (t > 0.0).then_some(t)
    };
    let hits_box = |lo: [f64; 3], hi: [f64; 3]| -> bool {
        let (mut t0, mut t1) = (0.0f64, f64::MAX);
        for k in 0..3 {
            if d[k].abs() < 1e-12 {
                if origin[k] < lo[k] || origin[k] > hi[k] {
                    return false;
                }
                continue;
            }
            let (a, b) = ((lo[k] - origin[k]) / d[k], (hi[k] - origin[k]) / d[k]);
            t0 = t0.max(a.min(b));
            t1 = t1.min(a.max(b));
        }
        t0 <= t1
    };
    let mut best: Option<(f64, usize)> = None;
    let mut take = |t: f64, part: usize| {
        if best.is_none_or(|b| t < b.0) {
            best = Some((t, part));
        }
    };
    for b in &scene.boxes {
        if !hits_box(b.lo, b.hi) {
            continue;
        }
        for (corners, _) in solid_faces(b) {
            for i in 1..corners.len().saturating_sub(1) {
                if let Some(t) = hit_tri(corners[0], corners[i], corners[i + 1]) {
                    take(t, b.part);
                }
            }
        }
    }
    for m in &scene.meshes {
        let Some((lo, hi)) = m.mesh.bounds() else { continue };
        if !hits_box(lo, hi) {
            continue;
        }
        let v: Vec<[f64; 3]> = m.mesh.vertices.iter().map(|p| p.map(f64::from)).collect();
        for t in &m.mesh.triangles {
            if let Some(tt) = hit_tri(v[t[0] as usize], v[t[1] as usize], v[t[2] as usize]) {
                take(tt, m.part);
            }
        }
    }
    let (t_part, part) = best?;
    // the board in front of the part (looking at a bottom part from above)
    for z in [0.0, BOARD_THICKNESS] {
        if d[2].abs() > 1e-12 {
            let t = (z - origin[2]) / d[2];
            if t > 0.0 && t < t_part - 1e-6 {
                let q = Point::new(origin[0] + t * d[0], origin[1] + t * d[1]);
                if pcb_core::geom::inside_polygon(&scene.outline, q) {
                    return None;
                }
            }
        }
    }
    Some(part)
}
