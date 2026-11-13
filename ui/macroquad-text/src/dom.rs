//! The DOM text sink — a web build hands its text to the browser.
//!
//! On wasm32 only, and on by default for a page that includes the plugin (`?text=canvas` opts
//! out; answered by
//! `js/text.js`), every run [`crate::draw_run`] would draw on the screen is
//! collected instead of rasterised, and once a frame [`flush`] ships them to the
//! page, where they become positioned spans over the canvas. macroquad keeps the
//! layout — where each run goes, how big, what colour, what clips it — and the
//! browser draws the glyphs: hinting, real font fallback, zoom, selection and
//! screen readers come with it.
//!
//! **Occlusion.** The spans sit above the whole canvas, so nothing the canvas
//! paints afterwards can hide them. A surface drawn over content says so with
//! [`cover`]: in draw order, it dims (a scrim) or hides and clips (a card, a
//! toast, a tape) the runs collected before it, and never those after it — the
//! same rule the text [`report`](crate::report) applies, through
//! [`crate::cover`]. The kit's sheets, wallet modal and toasts call it; an app's
//! own fixed overlays do too.
//!
//! Text drawn into a render target (a baked face, a capture, a pixel game's
//! overlay pass) is never sunk: it is part of a texture, not the screen. Native
//! builds never sink, so goldens and the text report draw as before.
//!
//! The frame goes over as one string through the miniquad plugin bridge (no
//! wasm-bindgen: miniquad's gl.js cannot host it). One run a line, its fields
//! tab-separated, positions normalised to the canvas (0..1) so the page scales
//! them by the canvas's CSS size:
//!
//! ```text
//! left  top  width  height  size  family  rrggbbaa  clip  text
//! ```
//!
//! `size` is the font size as a share of the canvas height, `family` is `m`
//! (monospace) or `p` (proportional), and `clip` is `l,t,r,b` normalised, or
//! `-` for none.

use macroquad::prelude::*;

use crate::report;
use crate::style::TextStyle;

/// The page script that draws the frames: stamp it into the bundle's `web/`.
pub const PLUGIN_JS: &str = include_str!("../js/text.js");

#[cfg(target_arch = "wasm32")]
mod imp {
    use std::cell::RefCell;
    use std::fmt::Write as _;

    use macroquad::prelude::*;
    use sapp_jsutils::JsObject;

    use crate::cover::{self, Effect};
    use crate::style::TextStyle;

    #[link(wasm_import_module = "env")]
    unsafe extern "C" {
        fn mqtext_enabled() -> u32;
        fn mqtext_font(family: JsObject, bytes: JsObject);
        fn mqtext_frame(frame: JsObject);
    }

    /// Plugin handshake for gl.js: the page script's `version` must match.
    #[unsafe(no_mangle)]
    extern "C" fn mqtext_crate_version() -> u32 {
        1
    }

    /// One run, in canvas fractions, until the frame ships.
    struct Run {
        rect: Rect,
        size: f32,
        mono: bool,
        rgb: [u8; 3],
        alpha: f32,
        clip: Option<Rect>,
        text: String,
        hidden: bool,
    }

    #[derive(Default)]
    struct Sink {
        /// Asked of the page once, on first use.
        enabled: Option<bool>,
        /// The app has handed the page its fonts. An app that never does keeps canvas text,
        /// whatever the page allows: the browser would otherwise draw it in a fallback face.
        registered: bool,
        runs: Vec<Run>,
        /// The `i`/`M` width pair of each registered monospace font, at 32.
        mono: Vec<(f32, f32)>,
    }

    thread_local! {
        static SINK: RefCell<Sink> = RefCell::new(Sink::default());
    }

    /// The page draws the text: it allows it (`text.js`, on unless `?text=canvas`) and the app has
    /// registered its fonts. Opt-in per app, so a kit change never moves an app that has not.
    pub fn active() -> bool {
        SINK.with_borrow_mut(|s| {
            s.registered
                && *s
                    .enabled
                    .get_or_insert_with(|| unsafe { mqtext_enabled() } == 1)
        })
    }

    pub fn register_font(font: &Font, bytes: &[u8], mono: bool) {
        if mono {
            let pair = signature(Some(font));
            SINK.with_borrow_mut(|s| s.mono.push(pair));
        }
        SINK.with_borrow_mut(|s| s.registered = true);
        if active() {
            let family = if mono { "mq-m" } else { "mq-p" };
            unsafe { mqtext_font(JsObject::string(family), JsObject::buffer(bytes)) };
        }
    }

    fn signature(font: Option<&Font>) -> (f32, f32) {
        (
            measure_text("i", font, 32, 1.0).width,
            measure_text("M", font, 32, 1.0).width,
        )
    }

    /// Whether the current pass draws to the screen rather than a texture.
    fn on_screen() -> bool {
        // SAFETY: macroquad's own escape hatch to its GL state, read between
        // draws on the drawing thread.
        let gl = unsafe { get_internal_gl() };
        gl.quad_gl.get_active_render_pass().is_none()
    }

    fn projection() -> Mat4 {
        // SAFETY: as above.
        unsafe { get_internal_gl() }.quad_gl.get_projection_matrix()
    }

    fn to_canvas(p: Mat4, x: f32, y: f32) -> Vec2 {
        let ndc = p.project_point3(vec3(x, y, 0.0));
        vec2((ndc.x + 1.0) * 0.5, (1.0 - ndc.y) * 0.5)
    }

    /// `r`, in the current camera's units, as a rectangle of canvas fractions.
    fn rect_to_canvas(p: Mat4, r: Rect) -> Rect {
        let a = to_canvas(p, r.x, r.y);
        let b = to_canvas(p, r.right(), r.bottom());
        let (lo, hi) = (a.min(b), a.max(b));
        Rect::new(lo.x, lo.y, hi.x - lo.x, hi.y - lo.y)
    }

    pub fn take(
        text: &str,
        style: &TextStyle,
        rect: Rect,
        size: f32,
        alpha: f32,
        clips: &[Rect],
    ) -> bool {
        if !active() || !on_screen() {
            return false;
        }
        let p = projection();
        let size_n = (to_canvas(p, 0.0, size).y - to_canvas(p, 0.0, 0.0).y).abs();
        let clip = clips.iter().fold(None::<Rect>, |acc, c| {
            let c = rect_to_canvas(p, *c);
            Some(match acc {
                Some(a) => a.intersect(c).unwrap_or(Rect::new(c.x, c.y, 0.0, 0.0)),
                None => c,
            })
        });
        let c = style.color;
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        SINK.with_borrow_mut(|s| {
            let mono = s.mono.contains(&signature(style.font.as_ref()));
            s.runs.push(Run {
                rect: rect_to_canvas(p, rect),
                size: size_n,
                mono,
                rgb: [byte(c.r), byte(c.g), byte(c.b)],
                alpha: c.a * alpha,
                clip,
                // A run is one line; a stray tab or newline would split the record.
                text: text
                    .chars()
                    .map(|ch| match ch {
                        '\t' | '\n' | '\r' => ' ',
                        ch => ch,
                    })
                    .collect(),
                hidden: false,
            });
        });
        true
    }

    pub fn cover(rect: Rect, opacity: f32) {
        if !active() || !on_screen() || opacity <= 0.0 {
            return;
        }
        let area = rect_to_canvas(projection(), rect);
        SINK.with_borrow_mut(|s| {
            for run in s.runs.iter_mut().filter(|r| !r.hidden) {
                match cover::effect(run.rect, run.clip, area, opacity) {
                    Effect::Clear => {}
                    Effect::Hidden => run.hidden = true,
                    Effect::Clip(strip) => run.clip = Some(strip),
                    Effect::Dim(k) => run.alpha *= k,
                }
            }
        });
    }

    pub fn flush() -> u32 {
        if !active() {
            return 0;
        }
        let runs = SINK.with_borrow_mut(|s| std::mem::take(&mut s.runs));
        let mut frame = String::with_capacity(runs.len() * 80);
        let mut sent = 0;
        for run in runs.iter().filter(|r| !r.hidden && r.alpha > 0.004) {
            let r = run.rect;
            let a = (run.alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
            let _ = write!(
                frame,
                "{:.5}\t{:.5}\t{:.5}\t{:.5}\t{:.5}\t{}\t{:02x}{:02x}{:02x}{a:02x}\t",
                r.x,
                r.y,
                r.w,
                r.h,
                run.size,
                if run.mono { 'm' } else { 'p' },
                run.rgb[0],
                run.rgb[1],
                run.rgb[2],
            );
            match run.clip {
                Some(c) => {
                    let _ = write!(
                        frame,
                        "{:.5},{:.5},{:.5},{:.5}\t",
                        c.x,
                        c.y,
                        c.right(),
                        c.bottom()
                    );
                }
                None => frame.push_str("-\t"),
            }
            frame.push_str(&run.text);
            frame.push('\n');
            sent += 1;
        }
        unsafe { mqtext_frame(JsObject::string(&frame)) };
        sent
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use macroquad::prelude::*;

    use crate::style::TextStyle;

    pub fn active() -> bool {
        false
    }

    pub fn register_font(_: &Font, _: &[u8], _: bool) {}

    pub fn take(_: &str, _: &TextStyle, _: Rect, _: f32, _: f32, _: &[Rect]) -> bool {
        false
    }

    pub fn cover(_: Rect, _: f32) {}

    pub fn flush() -> u32 {
        0
    }
}

/// Whether text goes to the page this run: a web build whose page asked.
pub fn active() -> bool {
    imp::active()
}

/// Give the page a font the app draws with, by its bytes, so the browser
/// draws the same face. `mono` marks the monospace one: the sink tells the
/// two apart by their glyph widths, and the page names them `mq-m` and `mq-p`.
/// A no-op natively and when the page has not asked for DOM text.
pub fn register_font(font: &Font, bytes: &[u8], mono: bool) {
    imp::register_font(font, bytes, mono);
}

/// A surface drawn over everything drawn before it, at `opacity`, over `rect`
/// in the current camera's units: a sheet's scrim and card, a toast, a fixed
/// tape. Call it as the surface is painted, before its own text.
///
/// Text drawn before it and under it is dimmed (a translucent cover) or hidden
/// and clipped (an opaque one), both in the DOM sink and in the text report;
/// text drawn after it is untouched. On a canvas the surface already paints
/// over what is under it, so this changes nothing a native capture shows.
pub fn cover(rect: Rect, opacity: f32) {
    imp::cover(rect, opacity);
    report::cover(rect, opacity);
}

/// Take a run for the page instead of drawing it: true when taken. `rect` is
/// the run's box and `size` its font size, both in the current camera's
/// units; `clips` the clips in force, outermost first.
pub(crate) fn take(
    text: &str,
    style: &TextStyle,
    rect: Rect,
    size: f32,
    alpha: f32,
    clips: &[Rect],
) -> bool {
    imp::take(text, style, rect, size, alpha, clips)
}

/// Ship this frame's runs to the page: once a frame, after the last text.
/// Returns how many runs went.
pub fn flush() -> u32 {
    imp::flush()
}
