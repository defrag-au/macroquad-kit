//! A [`Grade`] laid over a finished frame: one shader pass from the frame's target into another.
//!
//! The look of a dubbed VHS infomercial, in controls a reel names: black and white, contrast,
//! exposure, a vignette, scanlines, a worn tape (lines jittering sideways, grain, a tracking band
//! rolling down the picture) and colour bled sideways. The noise is a hash of the pixel and the
//! frame's number, never a random source, so a frame comes out the same every run.

use macroquad::prelude::*;

use super::file::Grade;

const VERTEX: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;

varying highp vec2 uv;
varying lowp vec4 color;

uniform mat4 Model;
uniform mat4 Projection;

void main() {
    gl_Position = Projection * Model * vec4(position, 1);
    color = color0 / 255.0;
    uv = texcoord;
}"#;

const FRAGMENT: &str = r#"#version 100
precision highp float;

varying highp vec2 uv;
varying lowp vec4 color;

uniform sampler2D Texture;
uniform vec2 Size;
uniform float Frame;
uniform float Mono;
uniform float Contrast;
uniform float Exposure;
uniform float Vignette;
uniform float Scan;
uniform float Tape;
uniform float Bleed;

float hash(vec2 p) {
    return fract(sin(dot(p, vec2(12.9898, 78.233))) * 43758.5453);
}

void main() {
    vec2 px = uv * Size;

    // A worn tape: each band of three lines slides sideways a little, differently every frame,
    // and a tracking band rolls down the picture every six seconds, tearing the lines in it.
    float line = floor(px.y / 3.0);
    float wobble = hash(vec2(line, Frame)) - 0.5;
    float roll = fract(Frame / 180.0);
    float band = smoothstep(0.05, 0.0, abs(uv.y - roll));
    float tear = band * 24.0 * hash(vec2(line, Frame + 7.0));
    vec2 at = uv + vec2(Tape * (wobble * 3.0 + tear) / Size.x, 0.0);

    // Colour bled sideways: red one way, blue the other.
    float off = Bleed * 6.0 / Size.x;
    vec3 c = vec3(
        texture2D(Texture, at + vec2(off, 0.0)).r,
        texture2D(Texture, at).g,
        texture2D(Texture, at - vec2(off, 0.0)).b
    );

    c *= exp2(Exposure);
    c = (c - 0.5) * (1.0 + Contrast) + 0.5;
    float luma = dot(c, vec3(0.299, 0.587, 0.114));
    c = mix(c, vec3(luma), Mono);

    // Grain, and the tracking band a shade brighter.
    c += (hash(px + vec2(Frame * 1.7, Frame)) - 0.5) * Tape * 0.16;
    c += band * Tape * 0.12;

    // Scanlines four pixels apart.
    float gap = 0.5 + 0.5 * cos(px.y * 3.14159265 * 0.5);
    c *= 1.0 - Scan * 0.45 * gap;

    float d = distance(uv, vec2(0.5));
    c *= 1.0 - Vignette * smoothstep(0.35, 0.8, d);

    gl_FragColor = vec4(clamp(c, 0.0, 1.0), 1.0) * color;
}"#;

/// The grade's shader, made once the window exists.
pub struct Grader {
    material: Material,
}

impl Grader {
    pub fn new() -> Result<Grader, String> {
        let material = load_material(
            ShaderSource::Glsl {
                vertex: VERTEX,
                fragment: FRAGMENT,
            },
            MaterialParams {
                uniforms: [
                    ("Size", UniformType::Float2),
                    ("Frame", UniformType::Float1),
                    ("Mono", UniformType::Float1),
                    ("Contrast", UniformType::Float1),
                    ("Exposure", UniformType::Float1),
                    ("Vignette", UniformType::Float1),
                    ("Scan", UniformType::Float1),
                    ("Tape", UniformType::Float1),
                    ("Bleed", UniformType::Float1),
                ]
                .into_iter()
                .map(|(name, kind)| UniformDesc::new(name, kind))
                .collect(),
                ..Default::default()
            },
        )
        .map_err(|e| format!("the reel's grade shader did not load: {e:?}"))?;
        Ok(Grader { material })
    }

    /// Draw `frame_texture` (a render target's, rows bottom-first) over `size` pixels under the
    /// current camera, through `grade`. `frame` seeds the noise.
    pub fn draw(&self, frame_texture: &Texture2D, size: Vec2, grade: &Grade, frame: u32) {
        let m = &self.material;
        m.set_uniform("Size", size);
        // Kept small, so the hash's sine stays precise.
        m.set_uniform("Frame", (frame % 1800) as f32);
        m.set_uniform("Mono", grade.mono);
        m.set_uniform("Contrast", grade.contrast);
        m.set_uniform("Exposure", grade.exposure);
        m.set_uniform("Vignette", grade.vignette);
        m.set_uniform("Scan", grade.scan);
        m.set_uniform("Tape", grade.tape);
        m.set_uniform("Bleed", grade.bleed);
        gl_use_material(m);
        draw_texture_ex(
            frame_texture,
            0.0,
            0.0,
            WHITE,
            DrawTextureParams {
                dest_size: Some(size),
                flip_y: true,
                ..Default::default()
            },
        );
        gl_use_default_material();
    }
}
