//! Drawing a [`Scene`] with macroquad, in the reel's own pixels, under whatever camera is set.
//!
//! Each item is drawn where it was placed, under a model matrix made from its pose: moved, then
//! turned and scaled about its own anchor, at its alpha. The game's frame arrives as a render
//! target's texture, upside down as every target's is. Text is laid out through the kit's
//! `RichText` and drawn through its layout, so a still's report lists every line of it.

use std::collections::BTreeMap;

use macroquad::models::{Mesh, Vertex, draw_mesh};
use macroquad::prelude::*;

use super::file::{
    Align, Anchor, Delta, Fill, Fit, Item, Look, Reel, Rgba, Row, Screen, Style, runs,
};
use super::timeline::{Layer, Scene};
use crate::clip::with_clip;
use crate::shape::{Finish, RoundedArt, fill_rounded, stroke_rounded};
use crate::text::{self, MeasureAt, RichText, TextLayout, TextStyle};

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

fn shape(screen: Screen, game: Option<(u32, u32)>) -> (u32, u32) {
    match screen {
        Screen::Blank { shape } => shape,
        Screen::Game => game.unwrap_or((390, 844)),
    }
}

/// The left edge of a block `width` wide whose `at` is aligned `align`.
pub fn aligned_left(align: Align, x: f32, width: f32) -> f32 {
    match align {
        Align::Left => x,
        Align::Centre => x - width / 2.0,
        Align::Right => x - width,
    }
}

/// The model matrix for `pose` on an item occupying `bounds` in a frame `frame_h` high, turned a
/// further `rot` degrees as placed.
pub fn model(pose: Delta, bounds: Rect, frame_h: f32, rot: f32) -> Mat4 {
    let anchor = bounds.center();
    let shift = vec2(
        pose.x,
        pose.y + pose.heights * bounds.h + pose.frames * frame_h,
    );
    Mat4::from_translation((anchor + shift).extend(0.0))
        * Mat4::from_rotation_z((pose.rot + rot).to_radians())
        * Mat4::from_scale(vec3(pose.scale, pose.scale, 1.0))
        * Mat4::from_translation((-anchor).extend(0.0))
}

fn color(c: Rgba, alpha: f32) -> Color {
    let [r, g, b, a] = c.unit();
    Color::new(r, g, b, a * alpha)
}

/// Text set in a style: its runs and their layout, `size` its block.
struct Set {
    rich: RichText,
    layout: TextLayout,
}

impl Set {
    fn size(&self) -> Vec2 {
        self.layout.size
    }

    fn draw(&self, top_left: Vec2, alpha: f32) {
        self.layout
            .draw(&self.rich, top_left, 1.0, alpha, |_, _| {});
    }

    /// What the text actually inks, from the layout's top-left: the line box's height leaves
    /// room for descenders, so "STEP 4" in capitals sits high in it. A plate is padded around
    /// this, and a `middle` anchor centres on it.
    fn ink(&self) -> Rect {
        let mut ink: Option<Rect> = None;
        for item in &self.layout.items {
            if let text::Placed::Text { text, style, pos } = item {
                let d = text::measure_run(text, &self.rich.styles()[style.0], 1.0);
                let r = Rect::new(pos.x, pos.y - d.offset_y, d.width, d.height);
                ink = Some(ink.map_or(r, |i| i.combine_with(r)));
            }
        }
        ink.unwrap_or(Rect::new(0.0, 0.0, self.size().x, self.size().y))
    }
}

/// Where a block of text goes for `at`: its layout's top-left, and the rectangle its plate or
/// border covers. Across, the plate spans the lines' width; up and down, the ink's height, so
/// the padding above and below the letters is the same.
fn place_block(style: &Style, set: &Set, at: Vec2, align: Align, anchor: Anchor) -> (Vec2, Rect) {
    let size = set.size();
    let ink = set.ink();
    let left = aligned_left(align, at.x, size.x);
    let top = match anchor {
        Anchor::Top => at.y,
        Anchor::Middle => at.y - (ink.y + ink.h / 2.0),
    };
    let pad = Vec2::from(style.pad);
    let back = Rect::new(
        left - pad.x,
        top + ink.y - pad.y,
        size.x + 2.0 * pad.x,
        ink.h + 2.0 * pad.y,
    );
    (vec2(left, top), back)
}

/// A sheet's rows, laid out in its width: each row's top within the sheet, and the whole height.
struct SheetLayout {
    rows: Vec<(f32, SetRow)>,
    height: f32,
    pad: Vec2,
    radius: f32,
}

enum SetRow {
    Line(Set, Align),
    Boxed(Set, Rgba, f32),
    Buttons(Vec<Set>, Rgba, f32),
}

/// What drawing a reel holds across frames: its images, fonts and styles, and the shader that
/// rounds a screen.
pub struct Painter {
    images: BTreeMap<String, Texture2D>,
    fonts: BTreeMap<String, Font>,
    styles: BTreeMap<String, Style>,
    art: RoundedArt,
}

impl Painter {
    /// Load the reel's images and fonts. A missing file is an error naming it, before anything
    /// is drawn.
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
        let mut fonts = BTreeMap::new();
        for (name, path) in &reel.fonts {
            let bytes = std::fs::read(path).map_err(|e| format!("font `{name}` ({path}): {e}"))?;
            let font =
                text::load_font(&bytes).map_err(|e| format!("font `{name}` ({path}): {e}"))?;
            fonts.insert(name.clone(), font);
        }
        Ok(Painter {
            images,
            fonts,
            styles: reel.styles.clone(),
            art: RoundedArt::new(),
        })
    }

    fn text_style(&self, style: &Style, ink: Rgba) -> TextStyle {
        let mut s = TextStyle::new(
            self.fonts.get(&style.font).cloned(),
            style.size,
            color(ink, 1.0),
        );
        if let Some(shadow) = style.shadow {
            s = s.shadow(color(shadow, 1.0), vec2(0.0, (style.size / 12.0).max(2.0)));
        }
        s
    }

    /// `body` set in `style`, wrapped at `width` (the style's own wrap if not given), its lines
    /// aligned within the block as the style says.
    fn set(&self, style: &str, body: &str, width: Option<f32>, ink: Option<Rgba>) -> Set {
        let style = &self.styles[style];
        let mut rich = RichText::new();
        let plain = rich.style(self.text_style(style, ink.unwrap_or(style.ink)));
        let em = rich.style(self.text_style(style, style.em.unwrap_or(style.ink)));
        for run in runs(body) {
            rich.text(&run.text, if run.em { em } else { plain });
        }
        let m = MeasureAt(1.0);
        let limit = width.or(style.wrap).unwrap_or(1.0e6);
        // Once to find the block's width, again to align the lines within it.
        let widest = rich.layout(&m, limit, text::Align::Start, style.gap).size.x;
        let align = match style.align {
            Align::Left => text::Align::Start,
            Align::Centre => text::Align::Center,
            Align::Right => text::Align::End,
        };
        // A hair over the widest line, so the widest line still fits when measured again: at
        // exactly its own width, rounding can push its last word onto a line of its own.
        let layout = rich.layout(&m, widest.max(1.0) + 0.5, align, style.gap);
        Set { rich, layout }
    }

    /// Draw `scene` over a frame of `size` pixels. `game` is the game's finished frame, if the
    /// shot has one.
    pub fn draw(&self, scene: &Scene, size: Vec2, game: Option<&Texture2D>) {
        let frame = Rect::new(0.0, 0.0, size.x, size.y);
        for layer in &scene.layers {
            self.layer(scene, layer, frame, game);
        }
        if let Some(veil) = scene.veil {
            draw_rectangle(0.0, 0.0, size.x, size.y, color(veil, 1.0));
        }
    }

    fn layer(&self, scene: &Scene, layer: &Layer, frame: Rect, game: Option<&Texture2D>) {
        let alpha = layer.pose.alpha.clamp(0.0, 1.0);
        let pose = layer.pose;
        let push = |bounds: Rect, rot: f32| {
            unsafe { get_internal_gl() }
                .quad_gl
                .push_model_matrix(model(pose, bounds, frame.h, rot));
        };
        let pop = || unsafe { get_internal_gl() }.quad_gl.pop_model_matrix();
        match &layer.item.look {
            Look::Fill(fill) => {
                push(frame, 0.0);
                draw_fill(fill, frame, alpha);
                pop();
            }
            Look::Image { image, fit } => {
                if let Some(texture) = self.images.get(image) {
                    push(frame, 0.0);
                    self.image(texture, frame, *fit, alpha);
                    pop();
                }
            }
            Look::View => {
                if let Some(texture) = game {
                    push(frame, 0.0);
                    let tint = Color::new(1.0, 1.0, 1.0, alpha);
                    self.art
                        .draw_target(texture, frame, 0.0, tint, Finish::NONE);
                    pop();
                }
            }
            Look::Phone {
                at,
                width,
                accent,
                screen,
                label,
            } => {
                let p = phone_layout(*at, *width, shape(*screen, scene.game));
                push(p.outer, 0.0);
                let shown = match screen {
                    Screen::Game => game,
                    Screen::Blank { .. } => None,
                };
                self.phone(&p, *accent, shown, alpha);
                if let Some(label) = label {
                    // Centred on the phone's top edge, whatever the chip style says.
                    let st = &self.styles[&label.style];
                    let set = self.set(&label.style, &label.text, None, None);
                    let at = vec2(p.outer.center().x, p.outer.y);
                    let placed = place_block(st, &set, at, Align::Centre, Anchor::Middle);
                    self.block(st, &set, placed, alpha);
                }
                pop();
            }
            Look::Text {
                style,
                at,
                rot,
                text,
            } => {
                let st = &self.styles[style];
                let set = self.set(style, text, None, None);
                let placed = place_block(st, &set, vec2(at[0], at[1]), st.align, st.anchor);
                push(placed.1, *rot);
                self.block(st, &set, placed, alpha);
                pop();
            }
            Look::Tap { at, size, ink } => {
                let centre = vec2(at[0], at[1]);
                let r = size / 2.0;
                push(Rect::new(centre.x - r, centre.y - r, *size, *size), 0.0);
                draw_circle(centre.x, centre.y, r, color(*ink, 0.25 * alpha));
                draw_circle_lines(
                    centre.x,
                    centre.y,
                    r,
                    (size / 15.0).max(2.0),
                    color(*ink, alpha),
                );
                pop();
            }
            Look::Sheet { fill, scrim, rows } => {
                self.sheet(scene, layer, frame, *fill, *scrim, rows, alpha);
            }
        }
    }

    /// Text placed by [`place_block`], with its plate or border if the style has one.
    fn block(&self, style: &Style, set: &Set, (origin, back): (Vec2, Rect), alpha: f32) {
        if let Some(plate) = style.plate {
            fill_rounded(back, style.radius.min(back.h / 2.0), color(plate, alpha));
        }
        if style.border > 0.0 {
            stroke_rounded(back, style.radius, style.border, color(style.ink, alpha));
        }
        set.draw(origin, alpha);
    }

    /// The parent phone of a sheet, and the parent's pose now.
    fn sheet_phone(&self, scene: &Scene, item: &Item) -> Option<(PhoneLayout, Delta)> {
        let parent = item.parent.as_deref()?;
        let host = scene
            .layers
            .iter()
            .find(|l| l.item.name.as_deref() == Some(parent))?;
        let Look::Phone {
            at, width, screen, ..
        } = &host.item.look
        else {
            return None;
        };
        Some((
            phone_layout(*at, *width, shape(*screen, scene.game)),
            host.pose,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn sheet(
        &self,
        scene: &Scene,
        layer: &Layer,
        frame: Rect,
        fill: Rgba,
        scrim: f32,
        rows: &[Row],
        alpha: f32,
    ) {
        let Some((phone, host)) = self.sheet_phone(scene, layer.item) else {
            return;
        };
        // Where the screen is now, moved with its phone (turns and scales aside).
        let shift = vec2(host.x, host.y + host.frames * frame.h);
        let screen = Rect::new(
            phone.screen.x + shift.x,
            phone.screen.y + shift.y,
            phone.screen.w,
            phone.screen.h,
        );
        let sheet = self.sheet_layout(rows, screen.w);
        let placed = Rect::new(
            screen.x,
            screen.bottom() - sheet.height,
            screen.w,
            sheet.height,
        );
        // The sheet's own slide, without its parent's (already in `screen`).
        let own = Delta {
            x: layer.pose.x - host.x,
            y: layer.pose.y - host.y,
            frames: layer.pose.frames - host.frames,
            ..layer.pose
        };
        // The scrim darkens as the sheet comes up: none a whole height down, all when home.
        let arrived = 1.0 - own.heights.clamp(0.0, 1.0);
        fill_rounded(
            screen,
            phone.screen_radius,
            Color::new(0.0, 0.0, 0.0, scrim * arrived * alpha),
        );
        with_clip(screen, || {
            unsafe { get_internal_gl() }
                .quad_gl
                .push_model_matrix(model(own, placed, frame.h, 0.0));
            fill_rounded(placed, sheet.radius, color(fill, alpha));
            for (top, row) in &sheet.rows {
                let y = placed.y + top;
                let inner = placed.w - 2.0 * sheet.pad.x;
                let x = placed.x + sheet.pad.x;
                match row {
                    SetRow::Line(set, align) => {
                        let left = match align {
                            Align::Left => x,
                            Align::Centre => x + (inner - set.size().x) / 2.0,
                            Align::Right => x + inner - set.size().x,
                        };
                        set.draw(vec2(left, y), alpha);
                    }
                    SetRow::Boxed(set, back, pad) => {
                        let r = Rect::new(x, y, inner, set.size().y + 2.0 * pad);
                        fill_rounded(r, pad * 0.8, color(*back, alpha));
                        set.draw(vec2(x + pad, y + pad), alpha);
                    }
                    SetRow::Buttons(sets, ink, pad) => {
                        let gap = sheet.pad.x * 0.5;
                        let n = sets.len() as f32;
                        let w = (inner - gap * (n - 1.0)) / n;
                        let h = sets.iter().map(|s| s.size().y).fold(0.0, f32::max) + 2.0 * pad;
                        for (i, set) in sets.iter().enumerate() {
                            let b = Rect::new(x + i as f32 * (w + gap), y, w, h);
                            let last = i + 1 == sets.len();
                            if last {
                                fill_rounded(b, h / 2.0, color(*ink, alpha));
                            }
                            stroke_rounded(b, h / 2.0, 3.0, color(*ink, alpha));
                            let at = vec2(
                                b.x + (w - set.size().x) / 2.0,
                                b.y + (h - set.size().y) / 2.0,
                            );
                            set.draw(at, alpha);
                        }
                    }
                }
            }
            unsafe { get_internal_gl() }.quad_gl.pop_model_matrix();
        });
    }

    /// A sheet's rows laid out `width` wide, its spacing in proportion to it.
    fn sheet_layout(&self, rows: &[Row], width: f32) -> SheetLayout {
        let k = width / 520.0;
        let pad = vec2(30.0, 34.0) * k;
        let gap = 20.0 * k;
        let inner = width - 2.0 * pad.x;
        let mut top = pad.y;
        let mut set_rows = Vec::new();
        for row in rows {
            let (row, height) = match row {
                Row::Line { style, text } => {
                    let set = self.set(style, text, Some(inner), None);
                    let h = set.size().y;
                    (SetRow::Line(set, self.styles[style].align), h)
                }
                Row::Boxed { style, text, fill } => {
                    let box_pad = 20.0 * k;
                    let set = self.set(style, text, Some(inner - 2.0 * box_pad), None);
                    let h = set.size().y + 2.0 * box_pad;
                    (SetRow::Boxed(set, *fill, box_pad), h)
                }
                Row::Buttons { style, labels } => {
                    let st = &self.styles[style];
                    // The last button is filled with the ink: its label is set in the sheet's
                    // fill colour, white on the ink.
                    let sets: Vec<Set> = labels
                        .iter()
                        .enumerate()
                        .map(|(i, l)| {
                            let ink = (i + 1 == labels.len()).then_some(Rgba::WHITE);
                            self.set(style, l, None, ink)
                        })
                        .collect();
                    let pad = 24.0 * k;
                    let h = sets.iter().map(|s| s.size().y).fold(0.0, f32::max) + 2.0 * pad;
                    (SetRow::Buttons(sets, st.ink, pad), h)
                }
            };
            set_rows.push((top, row));
            top += height + gap;
        }
        SheetLayout {
            rows: set_rows,
            height: top - gap + 44.0 * k,
            pad,
            radius: 36.0 * k,
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
        let still = model(Delta::NONE, b, 1000.0, 0.0);
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
            -8.0,
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
            0.0,
        );
        assert!(
            slid.transform_point3(centre)
                .abs_diff_eq(centre + vec3(0.0, 603.0, 0.0), 1e-3)
        );
    }

    #[test]
    fn alignment_places_a_block_by_its_left_middle_or_right() {
        assert_eq!(aligned_left(Align::Left, 540.0, 200.0), 540.0);
        assert_eq!(aligned_left(Align::Centre, 540.0, 200.0), 440.0);
        assert_eq!(aligned_left(Align::Right, 540.0, 200.0), 340.0);
    }
}
