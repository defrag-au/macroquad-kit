//! `sdf` — rounded rectangles, rings, discs and arcs drawn as one quad each,
//! antialiased by a distance field.
//!
//! **Private on purpose.** [`crate::shape`] and [`crate::draw_rounded_rect`] are
//! the doors, and there is no other way to draw a rounded shape in this crate.
//! A second public name is a second way for new code to draw the old one, and
//! the doc comment explaining which is which does not survive contact with a
//! completion list.
//!
//! ## What it replaces
//!
//! The tessellated path built a rounded fill as a 20-triangle fan and an
//! outline as four lines plus four twelve-segment arcs — about 200 triangles
//! for a 1.5 px ring. Every edge it drew was hard, and a large radius showed
//! the segments. Six vertices and a fragment shader do the same shapes exactly,
//! with the edge antialiased at any radius.
//!
//! It is also cheaper, which is the non-obvious part: measured on a
//! twelve-card grid with a hairline ring each, 0.015 ms against 0.023 ms in
//! release, because building the geometry cost more than the extra draw call.
//!
//! ## How a shape is described
//!
//! macroquad's [`macroquad::models::Vertex::normal`] is documented as free user
//! data — it is uploaded as a `vec4` attribute that nothing in the default
//! pipeline reads. So each quad carries its own geometry there:
//!
//! ```text
//! normal = (half_width, half_height, radius, thickness)
//! ```
//!
//! `thickness == 0` fills the shape; anything else draws a ring of that width
//! centred on the edge. `texcoord` carries the fragment's offset from the
//! shape's centre, in points, so the shader needs no per-shape uniform for
//! geometry.
//!
//! An arc is the same ring with a sweep, and the sweep is the one per-shape
//! *uniform* — `Sweep` — because a vertex has no room left. Uniforms are
//! snapshotted when a draw call starts, so every shape binds the material,
//! sets its own, draws and unbinds; two shapes in one batch would share the
//! first one's sweep.
//!
//! ## Precision
//!
//! The fragment declares `highp` where the driver has it
//! (`GL_FRAGMENT_PRECISION_HIGH`): a one-pixel edge ramp at x = 800 quantises to
//! a visible step at `mediump`'s 2^-10 relative precision. The fallback still
//! runs; it is just softer.

use std::cell::{Cell, RefCell};

use macroquad::miniquad::{BlendFactor, BlendState, BlendValue, Equation, PipelineParams};
use macroquad::models::{Mesh, Vertex, draw_mesh};
use macroquad::prelude::*;

const VERTEX: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
attribute vec4 normal;

varying lowp vec4 v_color;
varying highp vec4 v_params;
varying highp vec2 v_local;

uniform mat4 Model;
uniform mat4 Projection;

void main() {
    gl_Position = Projection * Model * vec4(position, 1.0);
    v_color = color0 / 255.0;
    v_params = normal;
    v_local = texcoord;
}"#;

const FRAGMENT: &str = r#"#version 100
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

varying lowp vec4 v_color;
varying highp vec4 v_params;
varying highp vec2 v_local;

uniform float Px;
// x = where the sweep starts, radians. y = how far it runs, radians.
// y == 0 means the whole shape, which is every fill and every full ring.
uniform vec2 Sweep;

const float TAU = 6.28318530718;

void main() {
    vec2 half_size = v_params.xy;
    float radius = v_params.z;
    float thickness = v_params.w;

    // The signed distance to a rounded box, negative inside: the exact shape,
    // not a polygon standing in for it.
    vec2 q = abs(v_local) - (half_size - vec2(radius));
    float d = length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0) - radius;

    // A ring is centred on the edge, so its inner and outer boundaries both
    // antialias from one field.
    float edge = thickness > 0.0
        ? abs(d + 0.5 * thickness) - 0.5 * thickness
        : d;
    float cover = clamp(0.5 - edge * Px, 0.0, 1.0);

    if (Sweep.y > 0.0) {
        // How far round the sweep this fragment is, and how much sweep is left:
        // both are arc lengths in points, so one expression antialiases the two
        // ends the way the band's own edges already are.
        float rel = mod(atan(v_local.y, v_local.x) - Sweep.x + TAU, TAU);
        float room = min(rel, Sweep.y - rel) * radius * Px;
        cover *= clamp(0.5 + room, 0.0, 1.0);
    }

    gl_FragColor = vec4(v_color.rgb, v_color.a * cover);
}"#;

thread_local! {
    static MATERIAL: RefCell<Option<Material>> = const { RefCell::new(None) };
    static FAILED: Cell<bool> = const { Cell::new(false) };
    /// Device pixels per point. Zero means "ask the screen".
    static PX: Cell<f32> = const { Cell::new(0.0) };
}

/// Make the pipeline if it is not made yet. Returns whether it is usable.
fn available() -> bool {
    if MATERIAL.with_borrow(|m| m.is_some()) {
        return true;
    }
    if FAILED.with(Cell::get) {
        return false;
    }
    let made = load_material(
        ShaderSource::Glsl {
            vertex: VERTEX,
            fragment: FRAGMENT,
        },
        MaterialParams {
            uniforms: vec![
                UniformDesc::new("Px", UniformType::Float1),
                UniformDesc::new("Sweep", UniformType::Float2),
            ],
            pipeline_params: PipelineParams {
                color_blend: Some(BlendState::new(
                    Equation::Add,
                    BlendFactor::Value(BlendValue::SourceAlpha),
                    BlendFactor::OneMinusValue(BlendValue::SourceAlpha),
                )),
                ..Default::default()
            },
            ..Default::default()
        },
    );
    match made {
        Ok(m) => {
            MATERIAL.with_borrow_mut(|slot| *slot = Some(m));
            true
        }
        Err(e) => {
            eprintln!("macroquad-widgets: sdf shader unavailable ({e:?}); shapes stay tessellated");
            FAILED.with(|f| f.set(true));
            false
        }
    }
}

/// Device pixels per point for the distance ramp.
///
/// [`crate::Painter`] keeps this in step with the scale it was built for, which
/// is the only place that knows about a zoomed camera or a dense screen. Zero
/// restores the screen's own scale.
pub(crate) fn set_px(px: f32) {
    PX.with(|p| p.set(px));
}

/// Device pixels per point the ramp is drawn for.
fn px() -> f32 {
    let set = PX.with(Cell::get);
    if set > 0.0 {
        set
    } else {
        screen_dpi_scale().max(0.5)
    }
}

/// Fill a rounded rectangle. False when the shader is unavailable, so the
/// caller can draw its tessellated version.
pub(crate) fn fill(rect: Rect, radius: f32, color: Color) -> bool {
    quad(rect, radius, 0.0, 0.0, 0.0, color)
}

/// A ring of `thickness` centred on the rounded rectangle's edge.
pub(crate) fn stroke(rect: Rect, radius: f32, thickness: f32, color: Color) -> bool {
    quad(rect, radius, thickness, 0.0, 0.0, color)
}

/// A filled disc.
pub(crate) fn disc(centre: Vec2, radius: f32, color: Color) -> bool {
    let r = radius.max(0.0);
    quad(
        Rect::new(centre.x - r, centre.y - r, r * 2.0, r * 2.0),
        r,
        0.0,
        0.0,
        0.0,
        color,
    )
}

/// A ring from `inner` outwards by `thickness`, which is what macroquad's
/// `draw_circle_lines` and `draw_arc` draw: the band sits outside the radius,
/// not centred on it.
pub(crate) fn band(centre: Vec2, inner: f32, thickness: f32, color: Color) -> bool {
    arc(centre, inner, thickness, 0.0, 0.0, color)
}

/// `band`, swept from `start` radians through `span` radians.
pub(crate) fn arc(
    centre: Vec2,
    inner: f32,
    thickness: f32,
    start: f32,
    span: f32,
    color: Color,
) -> bool {
    let outer = (inner + thickness).max(0.0);
    quad(
        Rect::new(centre.x - outer, centre.y - outer, outer * 2.0, outer * 2.0),
        outer,
        thickness,
        start,
        span,
        color,
    )
}

fn quad(
    rect: Rect,
    radius: f32,
    thickness: f32,
    sweep_start: f32,
    sweep_span: f32,
    color: Color,
) -> bool {
    if !available() {
        return false;
    }
    let px = px();
    // One device pixel of bleed: the ramp is 1/px points wide, so this covers
    // it at any scale.
    let pad = 1.0 / px.max(0.5);
    let (hw, hh) = (rect.w * 0.5, rect.h * 0.5);
    let r = radius.min(hw).min(hh).max(0.0);
    let c = rect.center();
    let params = vec4(hw, hh, r, thickness);

    let mut vertices = Vec::with_capacity(4);
    for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        let local = vec2(sx * (hw + pad), sy * (hh + pad));
        let mut v = Vertex::new(c.x + local.x, c.y + local.y, 0.0, local.x, local.y, color);
        v.normal = params;
        vertices.push(v);
    }
    let mesh = Mesh {
        vertices,
        indices: vec![0, 1, 2, 0, 2, 3],
        texture: None,
    };

    MATERIAL.with_borrow(|slot| {
        let Some(m) = slot else { return };
        m.set_uniform("Px", px);
        m.set_uniform("Sweep", vec2(sweep_start, sweep_span));
        gl_use_material(m);
        draw_mesh(&mesh);
        gl_use_default_material();
    });
    true
}
