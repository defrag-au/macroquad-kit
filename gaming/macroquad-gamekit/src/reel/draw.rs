//! Drawing a [`Scene`] with macroquad, in the reel's own pixels, under whatever camera is set.
//!
//! Each item is drawn where it was placed, under a model matrix made from its pose: moved, then
//! turned and scaled about its own anchor, at its alpha. The game's frame arrives as a render
//! target's texture, upside down as every target's is. Text is laid out through the kit's
//! `RichText` and drawn through its layout, so a still's report lists every line of it.

use std::collections::BTreeMap;

// Only for its `jpeg` feature, which macroquad's decoder then has: see the crate's manifest.
#[cfg(not(target_arch = "wasm32"))]
use image as _;

use macroquad::models::{Mesh, Vertex, draw_mesh};
use macroquad::prelude::*;

use super::file::{
    Align, Anchor, Area, Delta, Fill, Fit, Item, Label, Look, Reel, Rgba, Row, Screen, Style, runs,
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

/// A desktop browser: its toolbar over a page of the game's shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrowserLayout {
    pub outer: Rect,
    pub radius: f32,
    pub rim: f32,
    /// The toolbar, across the top inside the rim.
    pub bar: Rect,
    /// The page below it.
    pub page: Rect,
}

/// A browser `width` wide at `at`, its page the game's shape (a desktop's, if there is none).
pub fn browser_layout(at: [f32; 2], width: f32, game: Option<(u32, u32)>) -> BrowserLayout {
    let (gw, gh) = game.unwrap_or((1280, 800));
    let rim = (width * 0.003).max(1.0).round();
    let inner_w = width - 2.0 * rim;
    let bar_h = (width * 0.074).round();
    let page_h = (inner_w * gh as f32 / gw.max(1) as f32).round();
    let outer = Rect::new(at[0], at[1], width, bar_h + page_h + 2.0 * rim);
    BrowserLayout {
        outer,
        radius: width * 0.022,
        rim,
        bar: Rect::new(at[0] + rim, at[1] + rim, inner_w, bar_h),
        page: Rect::new(at[0] + rim, at[1] + rim + bar_h, inner_w, page_h),
    }
}

/// `text` with each blank line holding a zero-width space. The layout drops an empty line, so
/// paragraphs written apart (`"…\n\n…"`) would run together; a line holding a zero-width space
/// is a line's height of nothing. (Not a no-break space: that is whitespace to the layout, which
/// drops it with the line.)
pub fn keep_blank_lines(text: &str) -> String {
    text.split('\n')
        .map(|line| if line.is_empty() { "\u{200b}" } else { line })
        .collect::<Vec<_>>()
        .join("\n")
}

/// An [`Area`] as macroquad's `Rect`.
fn rect_of(a: Area) -> Rect {
    Rect::new(a.x, a.y, a.w, a.h)
}

/// `r` shrunk by `by` on every side.
fn inset(r: Rect, by: f32) -> Rect {
    Rect::new(r.x + by, r.y + by, r.w - 2.0 * by, r.h - 2.0 * by)
}

/// A soft glow of `rim`'s colour around a rounded `rect`: three widening, fading fills.
fn glow(rect: Rect, radius: f32, rim: Color, alpha: f32) {
    for (grow, a) in [(36.0, 0.05), (20.0, 0.07), (8.0, 0.1)] {
        let g = Rect::new(
            rect.x - grow,
            rect.y - grow,
            rect.w + 2.0 * grow,
            rect.h + 2.0 * grow,
        );
        fill_rounded(
            g,
            radius + grow,
            Color {
                a: a * alpha,
                ..rim
            },
        );
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

/// Draw under `m` as well as whatever is pushed already, until [`pop`]. macroquad composes a
/// pushed matrix with the one below it.
fn push(m: Mat4) {
    unsafe { get_internal_gl() }.quad_gl.push_model_matrix(m);
}

fn pop() {
    unsafe { get_internal_gl() }.quad_gl.pop_model_matrix();
}

/// The box in the frame around `rect`'s corners carried by `m`: a scissor for something drawn
/// under `m`, which can only be a rectangle.
pub fn carried_box(m: Mat4, rect: Rect) -> Rect {
    let corners = [
        rect.point(),
        vec2(rect.right(), rect.y),
        vec2(rect.x, rect.bottom()),
        vec2(rect.right(), rect.bottom()),
    ]
    .map(|p| m.transform_point3(p.extend(0.0)).truncate());
    let min = corners.iter().fold(corners[0], |a, &p| a.min(p));
    let max = corners.iter().fold(corners[0], |a, &p| a.max(p));
    Rect::new(min.x, min.y, max.x - min.x, max.y - min.y)
}

/// A phone's screen and its corner radius, if `item` is a phone.
fn screen_of(item: &Item, game: Option<(u32, u32)>) -> Option<(Rect, f32)> {
    let Look::Phone {
        at, width, screen, ..
    } = &item.look
    else {
        return None;
    };
    let p = phone_layout(*at, *width, shape(*screen, game));
    Some((p.screen, p.screen_radius))
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
        for run in runs(&keep_blank_lines(body)) {
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

    /// The rectangle `item` occupies as placed: its centre is what it turns and scales about,
    /// and its height the unit a `slide` moves in. `parent` is the item it is in, if any: a
    /// sheet sits at the bottom of its phone's screen.
    fn bounds(
        &self,
        item: &Item,
        parent: Option<&Item>,
        frame: Rect,
        game: Option<(u32, u32)>,
    ) -> Rect {
        match &item.look {
            Look::Fill(_) | Look::View => frame,
            Look::Image { area, .. } => area.map_or(frame, rect_of),
            Look::Rect { area, .. } => rect_of(*area),
            Look::Browser { at, width, .. } => browser_layout(*at, *width, game).outer,
            Look::Phone {
                at, width, screen, ..
            } => phone_layout(*at, *width, shape(*screen, game)).outer,
            Look::Text { style, at, text } => {
                let st = &self.styles[style];
                let set = self.set(style, text, None, None);
                place_block(st, &set, vec2(at[0], at[1]), st.align, st.anchor).1
            }
            Look::Tap { at, size, .. } => {
                Rect::new(at[0] - size / 2.0, at[1] - size / 2.0, *size, *size)
            }
            Look::Sheet { rows, .. } => match parent.and_then(|p| screen_of(p, game)) {
                Some((screen, _)) => {
                    let h = self.sheet_layout(rows, screen.w).height;
                    Rect::new(screen.x, screen.bottom() - h, screen.w, h)
                }
                None => frame,
            },
        }
    }

    /// What carries `layer` from where it was placed to where it is now: its parents' poses,
    /// each about its own centre, outermost first; and its own.
    fn matrices(&self, layer: &Layer, frame: Rect, game: Option<(u32, u32)>) -> (Mat4, Mat4) {
        let mut parents = Mat4::IDENTITY;
        let mut above = None;
        for (item, pose) in &layer.parents {
            let b = self.bounds(item, above, frame, game);
            parents *= model(*pose, b, frame.h, item.rot);
            above = Some(*item);
        }
        let b = self.bounds(layer.item, above, frame, game);
        (parents, model(layer.pose, b, frame.h, layer.item.rot))
    }

    fn layer(&self, scene: &Scene, layer: &Layer, frame: Rect, game: Option<&Texture2D>) {
        match layer.item.within {
            // Cut to its window in the frame, wherever its pose takes it.
            Some(window) => with_clip(rect_of(window), || self.posed(scene, layer, frame, game)),
            None => self.posed(scene, layer, frame, game),
        }
    }

    fn posed(&self, scene: &Scene, layer: &Layer, frame: Rect, game: Option<&Texture2D>) {
        let alpha = layer.alpha;
        if let Look::Sheet { fill, scrim, rows } = &layer.item.look {
            self.sheet(scene, layer, frame, *fill, *scrim, rows);
            return;
        }
        let (parents, own) = self.matrices(layer, frame, scene.game);
        push(parents * own);
        match &layer.item.look {
            Look::Fill(fill) => draw_fill(fill, frame, alpha),
            Look::Image {
                image,
                fit,
                area,
                radius,
            } => {
                if let Some(texture) = self.images.get(image) {
                    match area {
                        Some(area) => {
                            let tint = Color::new(1.0, 1.0, 1.0, alpha);
                            self.art.draw(texture, rect_of(*area), *radius, tint);
                        }
                        None => self.image(texture, frame, *fit, alpha),
                    }
                }
            }
            Look::Rect {
                area,
                fill,
                stroke,
                border,
                radius,
            } => {
                let r = rect_of(*area);
                if let Some(fill) = fill {
                    fill_rounded(r, *radius, color(*fill, alpha));
                }
                if let Some(stroke) = stroke {
                    stroke_rounded(r, *radius, *border, color(*stroke, alpha));
                }
            }
            Look::Browser {
                at,
                width,
                url,
                chips,
                accent,
            } => {
                let b = browser_layout(*at, *width, scene.game);
                self.browser(&b, url.as_ref(), chips, *accent, game, alpha);
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
                label,
            } => {
                let p = phone_layout(*at, *width, shape(*screen, scene.game));
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
            }
            Look::Text { style, at, text } => {
                let st = &self.styles[style];
                let set = self.set(style, text, None, None);
                let placed = place_block(st, &set, vec2(at[0], at[1]), st.align, st.anchor);
                self.block(st, &set, placed, alpha);
            }
            Look::Tap { at, size, ink } => {
                let r = size / 2.0;
                draw_circle(at[0], at[1], r, color(*ink, 0.25 * alpha));
                draw_circle_lines(at[0], at[1], r, (size / 15.0).max(2.0), color(*ink, alpha));
            }
            Look::Sheet { .. } => unreachable!("drawn above"),
        }
        pop();
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

    /// A sheet: the scrim over its phone's screen, then the sheet itself, cut to the screen.
    fn sheet(
        &self,
        scene: &Scene,
        layer: &Layer,
        frame: Rect,
        fill: Rgba,
        scrim: f32,
        rows: &[Row],
    ) {
        let alpha = layer.alpha;
        let Some((screen, radius)) = layer
            .parents
            .last()
            .and_then(|(phone, _)| screen_of(phone, scene.game))
        else {
            return;
        };
        let sheet = self.sheet_layout(rows, screen.w);
        let placed = Rect::new(
            screen.x,
            screen.bottom() - sheet.height,
            screen.w,
            sheet.height,
        );
        // With the phone where it is now: the scrim darkens as the sheet comes up, none when it
        // is a whole height down, all when it is home.
        let (parents, own) = self.matrices(layer, frame, scene.game);
        push(parents);
        let arrived = 1.0 - layer.pose.heights.clamp(0.0, 1.0);
        fill_rounded(
            screen,
            radius,
            Color::new(0.0, 0.0, 0.0, scrim * arrived * alpha),
        );
        pop();
        // A clip is a rectangle in the frame: the box around the screen's corners, carried by
        // the phone's pose.
        with_clip(carried_box(parents, screen), || {
            push(parents * own);
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
            pop();
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

    /// A browser: the glow and rim a phone has, the toolbar with its three dots, the address in
    /// a pill and the extensions' chips at its right end, and the page.
    fn browser(
        &self,
        b: &BrowserLayout,
        url: Option<&Label>,
        chips: &[Label],
        accent: Option<Rgba>,
        page: Option<&Texture2D>,
        alpha: f32,
    ) {
        let rim = accent.map_or(Color::from_rgba(44, 58, 48, 255), |c| color(c, 1.0));
        glow(b.outer, b.radius, rim, alpha);
        fill_rounded(b.outer, b.radius, Color { a: alpha, ..rim });
        let inner = inset(b.outer, b.rim);
        fill_rounded(inner, b.radius - b.rim, Color::new(0.10, 0.13, 0.11, alpha));
        if let Some(texture) = page {
            let tint = Color::new(1.0, 1.0, 1.0, alpha);
            self.art
                .draw_target(texture, b.page, b.radius * 0.6, tint, Finish::NONE);
        }
        // The dots.
        let mid = b.bar.y + b.bar.h / 2.0;
        let dot = b.bar.h * 0.12;
        for i in 0..3 {
            let x = b.bar.x + b.bar.h * 0.35 + i as f32 * dot * 3.4;
            draw_circle(x, mid, dot, Color::new(0.24, 0.29, 0.25, alpha));
        }
        // The chips, laid right to left from the bar's right end.
        let gap = b.bar.h * 0.15;
        let mut right = b.bar.right() - b.bar.h * 0.3;
        for chip in chips.iter().rev() {
            let st = &self.styles[&chip.style];
            let set = self.set(&chip.style, &chip.text, None, None);
            let at = vec2(right - st.pad[0], mid);
            let placed = place_block(st, &set, at, Align::Right, Anchor::Middle);
            right = placed.1.x - gap;
            self.block(st, &set, placed, alpha);
        }
        // The address, in a pill from after the dots to before the chips.
        if let Some(url) = url {
            let st = &self.styles[&url.style];
            let set = self.set(&url.style, &url.text, None, None);
            let left = b.bar.x + b.bar.h * 0.35 + dot * 3.4 * 2.0 + dot * 3.0;
            let pill = Rect::new(
                left,
                mid - b.bar.h * 0.3,
                (right - left).max(0.0),
                b.bar.h * 0.6,
            );
            fill_rounded(pill, pill.h / 2.0, Color::new(0.043, 0.059, 0.047, alpha));
            let at = vec2(pill.x + pill.h * 0.5, mid);
            let placed = place_block(st, &set, at, Align::Left, Anchor::Middle);
            set.draw(placed.0, alpha);
        }
    }

    /// A phone: a soft glow of its rim's colour, the rim, the body, and the screen.
    fn phone(&self, p: &PhoneLayout, accent: Option<Rgba>, screen: Option<&Texture2D>, alpha: f32) {
        let rim = accent.map_or(Color::from_rgba(44, 58, 48, 255), |c| color(c, 1.0));
        glow(p.outer, p.outer_radius, rim, alpha);
        fill_rounded(p.outer, p.outer_radius, Color { a: alpha, ..rim });
        let body = inset(p.outer, p.rim);
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
    fn a_blank_line_between_paragraphs_is_kept() {
        assert_eq!(keep_blank_lines("one\n\ntwo"), "one\n\u{200b}\ntwo");
        assert!(
            !'\u{200b}'.is_whitespace(),
            "the layout splits on whitespace"
        );
        assert_eq!(keep_blank_lines("one\ntwo"), "one\ntwo");
    }

    #[test]
    fn a_browsers_page_is_the_games_shape_under_its_toolbar() {
        let b = browser_layout([40.0, 520.0], 1000.0, Some((1280, 800)));
        assert_eq!(b.page.y, b.bar.bottom());
        assert!((b.page.h / b.page.w - 800.0 / 1280.0).abs() < 0.01);
        assert_eq!(b.outer.bottom(), b.page.bottom() + b.rim);
    }

    #[test]
    fn alignment_places_a_block_by_its_left_middle_or_right() {
        assert_eq!(aligned_left(Align::Left, 540.0, 200.0), 540.0);
        assert_eq!(aligned_left(Align::Centre, 540.0, 200.0), 440.0);
        assert_eq!(aligned_left(Align::Right, 540.0, 200.0), 340.0);
    }
}
