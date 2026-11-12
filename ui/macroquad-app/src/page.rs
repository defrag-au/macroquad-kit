//! `Page` — a screen that scrolls as a whole and is laid out top to bottom:
//! an asset's detail, a settings list, a market summary.
//!
//! Such a page only learns how tall it is by drawing, so its scroll's end is
//! the height it came to last frame. [`Page::begin`] moves it and says where
//! to start laying out; [`Page::end`] records where the layout finished.
//!
//! ```ignore
//! let (mut y, tap) = page.begin(body, &input, p.tap, dt);
//! let top = y;
//! with_clip(body, || { /* lay out from y, hit-testing with `tap` */ });
//! page.end(top, y);
//! ```
//!
//! A grid of many cards uses [`crate::ArtGrid`] instead, which knows its
//! height before it draws.

use macroquad::prelude::{Rect, Vec2};
use macroquad_widgets::{Scroll, ScrollInput};

/// Room left under the last thing on a page, so it does not sit on the edge.
const FOOT: f32 = 24.0;

/// See the [module docs](self).
#[derive(Clone, Debug, Default)]
pub struct Page {
    scroll: Scroll,
    /// How tall the layout came to, once [`Page::end`] has seen one. Unknown
    /// before the first frame, so a position set then is not clamped to 0.
    content: Option<f32>,
}

impl Page {
    pub fn new() -> Self {
        Self::default()
    }

    /// Move with this frame's `input`. Returns the top to lay out from, and
    /// `tap` if it is the page's: inside `area`, and not one that only caught
    /// the page coasting.
    pub fn begin(
        &mut self,
        area: Rect,
        input: &ScrollInput,
        tap: Option<Vec2>,
        dt: f32,
    ) -> (f32, Option<Vec2>) {
        let max = self.content.map(|content| (content - area.h).max(0.0));
        self.scroll.update(area, input, max, dt);
        let tap = self.scroll.filter_tap(tap).filter(|t| area.contains(*t));
        (area.y - self.scroll.offset(), tap)
    }

    /// The layout that began at `top` finished at `bottom`.
    pub fn end(&mut self, top: f32, bottom: f32) {
        self.content = Some(bottom - top + FOOT);
    }

    /// How far the page has scrolled: what an app bar's lift reads.
    pub fn offset(&self) -> f32 {
        self.scroll.offset()
    }

    /// Back to the top, as a tab tapped while on it does.
    pub fn scroll_home(&mut self) {
        self.scroll.set(0.0);
    }

    /// Jump to `offset` in the content, stopping any coast: a position
    /// restored from a route, or a capture that shows a scrolled screen.
    /// Clamped on the next [`Page::begin`], once the height is known — before
    /// the first [`Page::end`] it is kept, so a route can open mid-page.
    pub fn set(&mut self, offset: f32) {
        self.scroll.set(offset);
    }

    /// Scroll so `rect` — a caret's box in screen units, as
    /// [`crate::TextArea::caret_rect`] reports it — is inside the page above a
    /// band of `reserve` points at the bottom, which is the keyboard.
    ///
    /// Pass 0 when the page is already drawn in a body that [`crate::shell_layout`]
    /// shortened for the keyboard, and the keyboard's height when the page runs
    /// the whole window under it. Applied by the next [`Page::begin`]: the
    /// layout that reported the caret has already been drawn at this frame's
    /// offset.
    pub fn reveal(&mut self, area: Rect, rect: Rect, reserve: f32) {
        let top = area.y - self.offset();
        let content_top = rect.y - top;
        let content_bottom = rect.bottom() - top;
        let limit = (area.h - reserve).max(0.0);
        let shown = self.offset();
        if content_top < shown {
            self.scroll.set(content_top);
        } else if content_bottom > shown + limit {
            self.scroll.set(content_bottom - limit);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect {
        x: 0.0,
        y: 100.0,
        w: 390.0,
        h: 600.0,
    };

    fn drag(y: f32) -> ScrollInput {
        ScrollInput {
            drag: Some(Vec2::new(200.0, y)),
            pointer: Vec2::new(200.0, y),
            wheel: Vec2::ZERO,
        }
    }

    #[test]
    fn a_page_scrolls_as_far_as_it_was_tall_last_frame() {
        let mut page = Page::new();
        let (top, _) = page.begin(AREA, &ScrollInput::none(), None, 0.0);
        page.end(top, top + 1000.0);
        // Drag a long way up: it stops at the content's end.
        page.begin(AREA, &drag(650.0), None, 1.0 / 60.0);
        page.begin(AREA, &drag(-900.0), None, 1.0 / 60.0);
        assert_eq!(page.offset(), 1000.0 + FOOT - AREA.h);
        let (top, _) = page.begin(AREA, &drag(-900.0), None, 1.0 / 60.0);
        assert_eq!(top, AREA.y - page.offset());
    }

    #[test]
    fn a_page_can_be_placed_at_a_position() {
        let mut page = Page::new();
        let (top, _) = page.begin(AREA, &ScrollInput::none(), None, 0.0);
        page.end(top, top + 1000.0);
        page.set(200.0);
        let (top, _) = page.begin(AREA, &ScrollInput::none(), None, 0.0);
        assert_eq!(page.offset(), 200.0);
        assert_eq!(top, AREA.y - 200.0);
        // A position past the end is clamped when the frame lays out.
        page.set(10_000.0);
        page.begin(AREA, &ScrollInput::none(), None, 0.0);
        assert_eq!(page.offset(), 1000.0 + FOOT - AREA.h);
    }

    #[test]
    fn a_position_set_before_the_first_frame_is_kept() {
        let mut page = Page::new();
        page.set(200.0);
        // The height is not known yet, so the offset is not clamped to 0.
        let (top, _) = page.begin(AREA, &ScrollInput::none(), None, 0.0);
        assert_eq!(page.offset(), 200.0);
        assert_eq!(top, AREA.y - 200.0);
        // Once the layout has reported its height, a set past the end clamps.
        page.end(top, top + 1000.0);
        page.set(10_000.0);
        page.begin(AREA, &ScrollInput::none(), None, 0.0);
        assert_eq!(page.offset(), 1000.0 + FOOT - AREA.h);
    }

    #[test]
    fn reveal_brings_a_caret_above_the_keyboard() {
        let mut page = Page::new();
        let (top, _) = page.begin(AREA, &ScrollInput::none(), None, 0.0);
        page.end(top, top + 1000.0);
        // A caret 400 into the content, on a page showing 300 above the band.
        page.reveal(AREA, Rect::new(0.0, 500.0, 1.5, 17.0), 300.0);
        let (top, _) = page.begin(AREA, &ScrollInput::none(), None, 0.0);
        assert_eq!(page.offset(), 117.0, "the caret's bottom sits on the band");
        // A caret that scrolled above the page brings it back down.
        page.reveal(AREA, Rect::new(0.0, top + 20.0, 1.5, 17.0), 300.0);
        page.begin(AREA, &ScrollInput::none(), None, 0.0);
        assert_eq!(page.offset(), 20.0);
    }

    #[test]
    fn a_short_page_does_not_scroll_and_takes_taps_inside_only() {
        let mut page = Page::new();
        let (top, _) = page.begin(AREA, &ScrollInput::none(), None, 0.0);
        page.end(top, top + 200.0);
        let (_, inside) = page.begin(
            AREA,
            &ScrollInput::none(),
            Some(Vec2::new(10.0, 300.0)),
            0.0,
        );
        assert!(inside.is_some());
        let (_, outside) = page.begin(AREA, &ScrollInput::none(), Some(Vec2::new(10.0, 50.0)), 0.0);
        assert!(
            outside.is_none(),
            "a tap on the bar above is not the page's"
        );
        assert_eq!(page.offset(), 0.0);
    }
}
