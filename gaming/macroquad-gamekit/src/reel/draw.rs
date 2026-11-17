//! Drawing a [`Scene`] with macroquad, in the reel's own pixels, under whatever camera is set.
//!
//! Each item is drawn where it was placed, under a model matrix made from its pose: moved, then
//! turned and scaled about its own anchor, at its alpha. The game's frame arrives as a render
//! target's texture, upside down as every target's is.

use std::collections::BTreeMap;

use macroquad::models::{Mesh, Vertex, draw_mesh};
use macroquad::prelude::*;

use super::file::{Delta, Fill, Fit, Look, Reel, Rgba, Screen};
use super::timeline::Scene;
use crate::shape::{Finish, RoundedArt, fill_rounded};

/// A phone's frame around a screen of a given shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhoneLayout {
    /// The whole phone.
    pub outer: Rect,
    pub outer_radius: f32,
    /// The rim: the outer edge's width.
    pub rim: f32,
    /// The screen inside it, the shape it was given.
    pub screen: Rect,
    pub screen_radius: f32,
}

/// A phone `width` wide at `at`, around a screen of `shape` points' proportions. The proportions
/// are a modern phone's: a thin bezel, generous corners.
pub fn phone_layout(at: [f32; 2], width: f32, shape: (u32, u32)) -> PhoneLayout {
    let bezel = (width * 0.025).round();
    let rim = (width * 0.008).max(1.0).round();
    let screen_w = width - 2.0 * bezel;
    let screen_h = (screen_w * shape.1 as f32 / shape.0.max(1) as f32).round();
    let outer = Rect::new(at[0], at[1], width, screen_h + 2.0 * bezel);
    PhoneLayout {
        outer,
        outer_radius: width * 0.117,
        rim,
        screen: Rect::new(at[0] + bezel, at[1] + bezel, screen_w, screen_h),
        screen_radius: width * 0.095,
    }
}

/// The rectangle an item occupies as placed: the frame, or its phone. Its centre is the anchor
/// it turns and scales about, and its height the unit a `slide` moves in.
pub fn bounds(look: &Look, frame: Rect, game: Option<(u32, u32)>) -> Rect {
    match look {
        Look::Fill(_) | Look::Image { .. } | Look::View => frame,
        Look::Phone {
            at, width, screen, ..
        } => phone_layout(*at, *width, shape(*screen, game)).outer,
    }
}

fn shape(screen: Screen, game: Option<(u32, u32)>) -> (u32, u32) {
    match screen {
        Screen::Blank { shape } => shape,
        Screen::Game => game.unwrap_or((390, 844)),
    }
}

/// The model matrix for `pose` on an item occupying `bounds` in a frame `frame_h` high.
pub fn model(pose: Delta, bounds: Rect, frame_h: f32) -> Mat4 {
    let anchor = bounds.center();
    let shift = vec2(
        pose.x,
        pose.y + pose.heights * bounds.h + pose.frames * frame_h,
    );
    Mat4::from_translation((anchor + shift).extend(0.0))
        * Mat4::from_rotation_z(pose.rot.to_radians())
        * Mat4::from_scale(vec3(pose.scale, pose.scale, 1.0))
        * Mat4::from_translation((-anchor).extend(0.0))
}

fn color(c: Rgba, alpha: f32) -> Color {
    let [r, g, b, a] = c.unit();
    Color::new(r, g, b, a * alpha)
}

/// What drawing a reel holds across frames: its images, and the shader that rounds a screen.
pub struct Painter {
    images: BTreeMap<String, Texture2D>,
    art: RoundedArt,
}

impl Painter {
    /// Load the reel's images. A missing image is an error naming it, before anything is drawn.
    pub fn load(reel: &Reel) -> Result<Painter, String> {
        let mut images = BTreeMap::new();
        for (name, path) in &reel.images {
            let bytes = std::fs::read(path).map_err(|e| format!("image `{name}` ({path}): {e}"))?;
            let image = Image::from_file_with_format(&bytes, None)
                .map_err(|e| format!("image `{name}` ({path}): {e}"))?;
            let texture = Texture2D::from_image(&image);
            texture.set_filter(FilterMode::Linear);
            images.insert(name.clone(), texture);
        }
        Ok(Painter {
            images,
            art: RoundedArt::new(),
        })
    }

    /// Draw `scene` over a frame of `size` pixels. `game` is the game's finished frame, if the
    /// shot has one.
    pub fn draw(&self, scene: &Scene, size: Vec2, game: Option<&Texture2D>) {
        let frame = Rect::new(0.0, 0.0, size.x, size.y);
        for layer in &scene.layers {
            let look = &layer.item.look;
            let placed = bounds(look, frame, scene.game);
            let gl = unsafe { get_internal_gl() }.quad_gl;
            gl.push_model_matrix(model(layer.pose, placed, size.y));
            let alpha = layer.pose.alpha.clamp(0.0, 1.0);
            match look {
                Look::Fill(fill) => draw_fill(fill, frame, alpha),
                Look::Image { image, fit } => {
                    if let Some(texture) = self.images.get(image) {
                        self.image(texture, frame, *fit, alpha);
                    }
                }
                Look::View => {
                    if let Some(texture) = game {
                        let tint = Color::new(1.0, 1.0, 1.0, alpha);
                        self.art
                            .draw_target(texture, frame, 0.0, tint, Finish::NONE);
                    }
                }
                Look::Phone {
                    at,
                    width,
                    accent,
                    screen,
                    ..
                } => {
                    let p = phone_layout(*at, *width, shape(*screen, scene.game));
                    let screen = match screen {
                        Screen::Game => game,
                        Screen::Blank { .. } => None,
                    };
                    self.phone(&p, *accent, screen, alpha);
                }
            }
            unsafe { get_internal_gl() }.quad_gl.pop_model_matrix();
        }
        if let Some(veil) = scene.veil {
            draw_rectangle(0.0, 0.0, size.x, size.y, color(veil, 1.0));
        }
    }

    fn image(&self, texture: &Texture2D, frame: Rect, fit: Fit, alpha: f32) {
        let tint = Color::new(1.0, 1.0, 1.0, alpha);
        match fit {
            Fit::Cover => self.art.draw(texture, frame, 0.0, tint),
            Fit::Contain => {
                let tex = texture.size();
                let scale = (frame.w / tex.x).min(frame.h / tex.y);
                let shown = tex * scale;
                let at = frame.point() + (frame.size() - shown) / 2.0;
                draw_texture_ex(
                    texture,
                    at.x,
                    at.y,
                    tint,
                    DrawTextureParams {
                        dest_size: Some(shown),
                        ..Default::default()
                    },
                );
            }
        }
    }

    /// A phone: a soft glow of its rim's colour, the rim, the body, and the screen.
    fn phone(&self, p: &PhoneLayout, accent: Option<Rgba>, screen: Option<&Texture2D>, alpha: f32) {
        let rim = accent.map_or(Color::from_rgba(44, 58, 48, 255), |c| color(c, 1.0));
        for (grow, a) in [(36.0, 0.05), (20.0, 0.07), (8.0, 0.1)] {
            let g = Rect::new(
                p.outer.x - grow,
                p.outer.y - grow,
                p.outer.w + 2.0 * grow,
                p.outer.h + 2.0 * grow,
            );
            fill_rounded(
                g,
                p.outer_radius + grow,
                Color {
                    a: a * alpha,
                    ..rim
                },
            );
        }
        fill_rounded(p.outer, p.outer_radius, Color { a: alpha, ..rim });
        let body = Rect::new(
            p.outer.x + p.rim,
            p.outer.y + p.rim,
            p.outer.w - 2.0 * p.rim,
            p.outer.h - 2.0 * p.rim,
        );
        fill_rounded(
            body,
            p.outer_radius - p.rim,
            Color::new(0.043, 0.059, 0.047, alpha),
        );
        match screen {
            Some(texture) => self.art.draw_target(
                texture,
                p.screen,
                p.screen_radius,
                Color::new(1.0, 1.0, 1.0, alpha),
                Finish::NONE,
            ),
            None => fill_rounded(
                p.screen,
                p.screen_radius,
                Color::new(0.16, 0.16, 0.16, alpha),
            ),
        }
    }
}

/// A fill over `frame`, built from a mesh with a colour at each vertex.
fn draw_fill(fill: &Fill, frame: Rect, alpha: f32) {
    match fill {
        Fill::Solid(c) => draw_rectangle(frame.x, frame.y, frame.w, frame.h, color(*c, alpha)),
        Fill::Linear(stops) => {
            let n = stops.len() - 1;
            let mut vertices = Vec::new();
            let mut indices = Vec::new();
            for (i, stop) in stops.iter().enumerate() {
                let y = frame.y + frame.h * i as f32 / n as f32;
                let c = color(*stop, alpha);
                vertices.push(Vertex::new(frame.x, y, 0.0, 0.0, 0.0, c));
                vertices.push(Vertex::new(frame.right(), y, 0.0, 0.0, 0.0, c));
            }
            for i in 0..n as u16 {
                let k = i * 2;
                indices.extend_from_slice(&[k, k + 1, k + 2, k + 1, k + 3, k + 2]);
            }
            draw_mesh(&Mesh {
                vertices,
                indices,
                texture: None,
            });
        }
        Fill::Radial { inner, outer } => {
            // A fan from the centre to a ring at 70% of the corner distance, then a band out to
            // well past the corners in the outer colour.
            let centre = frame.center();
            let reach = (frame.size() / 2.0).length();
            let (ci, co) = (color(*inner, alpha), color(*outer, alpha));
            const SEGMENTS: u16 = 64;
            let mut vertices = vec![Vertex::new(centre.x, centre.y, 0.0, 0.0, 0.0, ci)];
            let mut indices = Vec::new();
            for i in 0..SEGMENTS {
                let a = std::f32::consts::TAU * i as f32 / SEGMENTS as f32;
                let dir = vec2(a.cos(), a.sin());
                let ring = centre + dir * reach * 0.7;
                let far = centre + dir * reach * 1.5;
                vertices.push(Vertex::new(ring.x, ring.y, 0.0, 0.0, 0.0, co));
                vertices.push(Vertex::new(far.x, far.y, 0.0, 0.0, 0.0, co));
            }
            for i in 0..SEGMENTS {
                let (r0, f0) = (1 + 2 * i, 2 + 2 * i);
                let next = (i + 1) % SEGMENTS;
                let (r1, f1) = (1 + 2 * next, 2 + 2 * next);
                indices.extend_from_slice(&[0, r0, r1, r0, f0, r1, r1, f0, f1]);
            }
            draw_mesh(&Mesh {
                vertices,
                indices,
                texture: None,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_phone_is_its_screens_shape_inside_a_thin_bezel() {
        let p = phone_layout([266.0, 256.0], 548.0, (390, 844));
        assert_eq!(p.screen.w, 548.0 - 2.0 * 14.0);
        let shape = p.screen.h / p.screen.w;
        assert!((shape - 844.0 / 390.0).abs() < 0.01, "{shape}");
        assert_eq!(p.outer.h, p.screen.h + 28.0);
        assert!(p.screen_radius < p.outer_radius);
    }

    #[test]
    fn a_pose_turns_and_scales_about_the_items_centre_then_moves_it() {
        let b = Rect::new(100.0, 100.0, 200.0, 100.0);
        let centre = b.center().extend(0.0);
        let still = model(Delta::NONE, b, 1000.0);
        assert!(
            still
                .transform_point3(vec3(5.0, 7.0, 0.0))
                .abs_diff_eq(vec3(5.0, 7.0, 0.0), 1e-4)
        );
        let spun = model(
            Delta {
                rot: 90.0,
                scale: 2.0,
                ..Delta::NONE
            },
            b,
            1000.0,
        );
        assert!(
            spun.transform_point3(centre).abs_diff_eq(centre, 1e-3),
            "the anchor stays put"
        );
        let slid = model(
            Delta {
                heights: 1.0,
                frames: 0.5,
                y: 3.0,
                ..Delta::NONE
            },
            b,
            1000.0,
        );
        assert!(
            slid.transform_point3(centre)
                .abs_diff_eq(centre + vec3(0.0, 603.0, 0.0), 1e-3)
        );
    }
}
