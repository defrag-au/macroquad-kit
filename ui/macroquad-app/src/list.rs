//! `List` — a scrolling column of fixed-height rows, of which only the rows in
//! view are ever visited: a thousand settings rows cost a frame what a dozen
//! does.
//!
//! The list is geometry and a [`Scroll`]; the host draws each row. Unlike
//! [`crate::ArtGrid`], a row holds whatever the host draws — a setting, a
//! trait, a search result — and the list owns the scroll, the rows in view and
//! the tap that lands on one.
//!
//! ```ignore
//! let geo = self.list.update(body, &input, rows.len(), spec, frame.dt);
//! self.list.draw(body, &geo, rows.len(), c.border, |i, rect| row(&p, &rows[i], rect));
//! if let Some(i) = self.list.hit(body, &geo, rows.len(), tap) { open(i) }
//! ```
//!
//! While the list moves, a thin thumb on its right edge says where in the whole
//! the reader is, and fades once it settles.

use std::ops::Range;

use macroquad::prelude::*;
use macroquad_gamekit::clip::with_clip;
use macroquad_widgets::{Scroll, ScrollInput, fill_rounded, theme::with_alpha};

/// The list's shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ListSpec {
    /// One row's height, in points.
    pub row_h: f32,
    /// Between rows, and around the list's edge when non-zero.
    pub gap: f32,
    /// A header above the first row, drawn by the host, that scrolls away.
    pub header: f32,
}

impl Default for ListSpec {
    fn default() -> Self {
        Self {
            row_h: 56.0,
            gap: 0.0,
            header: 0.0,
        }
    }
}

/// The list laid out for a count.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ListGeometry {
    /// The whole content's height, header included.
    pub content_h: f32,
    row_h: f32,
    gap: f32,
    header: f32,
}

impl ListGeometry {
    /// Lay out `count` rows.
    pub fn new(count: usize, spec: ListSpec) -> Self {
        Self {
            content_h: spec.header + spec.gap + count as f32 * (spec.row_h + spec.gap),
            row_h: spec.row_h,
            gap: spec.gap,
            header: spec.header,
        }
    }

    /// Where row `i` is, in content coordinates (0 at the header's top).
    pub fn row_in_content(&self, width: f32, i: usize) -> Rect {
        Rect::new(
            0.0,
            self.header + self.gap + i as f32 * (self.row_h + self.gap),
            width,
            self.row_h,
        )
    }

    /// The rows overlapping content from `top` to `top + h`.
    fn rows_in(&self, top: f32, h: f32) -> Range<usize> {
        let step = self.row_h + self.gap;
        let first = ((top - self.header - self.gap) / step).floor().max(0.0) as usize;
        let last = ((top + h - self.header) / step).ceil().max(0.0) as usize;
        first..last.max(first)
    }
}

/// See the [module docs](self).
#[derive(Clone, Debug, Default)]
pub struct List {
    scroll: Scroll,
    /// Seconds since the list last moved: how faded its thumb is.
    still: f32,
}

/// How long the thumb stays after the list stops.
const THUMB_HOLD: f32 = 0.9;
const THUMB_FADE: f32 = 0.3;

impl List {
    pub fn new() -> Self {
        Self {
            scroll: Scroll::new(),
            still: THUMB_HOLD + THUMB_FADE,
        }
    }

    /// The list's scroll: its offset is what an app bar's `raised` reads.
    pub fn scroll(&self) -> &Scroll {
        &self.scroll
    }

    /// Back to the top, as a new filter or a tab tapped twice does.
    pub fn to_top(&mut self) {
        self.scroll.set(0.0);
    }

    /// Advance a frame: lay the list out for `area` and move it with `input`.
    /// Behind a sheet, pass `ScrollInput::none()`.
    pub fn update(
        &mut self,
        area: Rect,
        input: &ScrollInput,
        count: usize,
        spec: ListSpec,
        dt: f32,
    ) -> ListGeometry {
        let geo = ListGeometry::new(count, spec);
        let moved = self
            .scroll
            .update(area, input, Some((geo.content_h - area.h).max(0.0)), dt);
        self.still = if moved || self.scroll.held() {
            0.0
        } else {
            self.still + dt
        };
        geo
    }

    /// The tap, unless it only caught the list coasting. Hit-test with this.
    pub fn filter_tap(&self, tap: Option<Vec2>) -> Option<Vec2> {
        self.scroll.filter_tap(tap)
    }

    /// The rows in view, plus `overscan` rows either side.
    pub fn visible(
        &self,
        area: Rect,
        geo: &ListGeometry,
        count: usize,
        overscan: usize,
    ) -> Range<usize> {
        let rows = geo.rows_in(self.scroll.offset(), area.h);
        let first = rows.start.saturating_sub(overscan);
        let last = (rows.end + overscan).min(count);
        first.min(last)..last
    }

    /// Where row `i` is on the stage this frame.
    pub fn row(&self, area: Rect, geo: &ListGeometry, i: usize) -> Rect {
        let r = geo.row_in_content(area.w, i);
        Rect::new(area.x + r.x, area.y + r.y - self.scroll.offset(), r.w, r.h)
    }

    /// Where the header is on the stage this frame.
    pub fn header(&self, area: Rect, spec: ListSpec) -> Rect {
        Rect::new(area.x, area.y - self.scroll.offset(), area.w, spec.header)
    }

    /// Draw the rows in view, each by `row`, clipped to `area`, then the thumb
    /// while the list moves. The header is the host's to draw, at
    /// [`List::header`], inside the same clip if it wants one.
    pub fn draw(
        &self,
        area: Rect,
        geo: &ListGeometry,
        count: usize,
        thumb: Color,
        mut row: impl FnMut(usize, Rect),
    ) {
        with_clip(area, || {
            for i in self.visible(area, geo, count, 0) {
                row(i, self.row(area, geo, i));
            }
        });
        let fade = 1.0 - ((self.still - THUMB_HOLD) / THUMB_FADE).clamp(0.0, 1.0);
        if fade > 0.0 && geo.content_h > area.h {
            let h = (area.h * area.h / geo.content_h).max(36.0);
            let max = geo.content_h - area.h;
            let y = area.y + (area.h - h) * (self.scroll.offset() / max).clamp(0.0, 1.0);
            let bar = Rect::new(area.right() - 5.0, y, 3.0, h);
            fill_rounded(bar, 1.5, with_alpha(thumb, 0.55 * fade));
        }
    }

    /// The row under `tap` (already through [`List::filter_tap`]).
    pub fn hit(
        &self,
        area: Rect,
        geo: &ListGeometry,
        count: usize,
        tap: Option<Vec2>,
    ) -> Option<usize> {
        let tap = tap.filter(|t| area.contains(*t))?;
        self.visible(area, geo, count, 0)
            .find(|&i| self.row(area, geo, i).contains(tap))
    }

    /// Scroll so row `i` is in view, as a restored selection is.
    pub fn reveal(&mut self, area: Rect, geo: &ListGeometry, i: usize) {
        let r = geo.row_in_content(area.w, i);
        let top = self.scroll.offset();
        if r.y < top {
            self.scroll.set(r.y - geo.gap);
        } else if r.bottom() > top + area.h {
            self.scroll.set(r.bottom() + geo.gap - area.h);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PHONE: Rect = Rect {
        x: 0.0,
        y: 103.0,
        w: 390.0,
        h: 600.0,
    };

    fn rows(row_h: f32) -> ListSpec {
        ListSpec {
            row_h,
            ..ListSpec::default()
        }
    }

    #[test]
    fn only_the_rows_in_view_are_visited() {
        let spec = rows(72.0);
        let list = List::new();
        let geo = ListGeometry::new(10_000, spec);
        // 600 points of 72-point rows: nine rows, the last one part seen.
        assert_eq!(list.visible(PHONE, &geo, 10_000, 0), 0..9);
        let near = list.visible(PHONE, &geo, 10_000, 2);
        assert_eq!(near, 0..11, "and two more rows below to load ahead");
    }

    #[test]
    fn the_last_row_is_cut_to_the_count() {
        let spec = rows(72.0);
        let list = List::new();
        let geo = ListGeometry::new(3, spec);
        assert_eq!(list.visible(PHONE, &geo, 3, 4), 0..3);
        assert_eq!(list.visible(PHONE, &geo, 0, 4), 0..0);
    }

    #[test]
    fn a_header_pushes_the_rows_down_and_scrolls_away() {
        let spec = ListSpec {
            header: 120.0,
            ..rows(72.0)
        };
        let mut list = List::new();
        let geo = ListGeometry::new(50, spec);
        assert_eq!(list.row(PHONE, &geo, 0).y, PHONE.y + 120.0 + spec.gap);
        list.scroll.set(400.0);
        list.update(PHONE, &ScrollInput::none(), 50, spec, 0.0);
        assert_eq!(list.header(PHONE, spec).y, PHONE.y - 400.0);
    }

    #[test]
    fn a_tap_finds_its_row() {
        let spec = rows(72.0);
        let list = List::new();
        let geo = ListGeometry::new(20, spec);
        let second = list.row(PHONE, &geo, 1);
        assert_eq!(list.hit(PHONE, &geo, 20, Some(second.center())), Some(1));
        let above = vec2(second.center().x, PHONE.y - 10.0);
        assert_eq!(list.hit(PHONE, &geo, 20, Some(above)), None, "above it");
    }

    #[test]
    fn reveal_scrolls_a_row_into_view() {
        let spec = rows(72.0);
        let mut list = List::new();
        let geo = ListGeometry::new(200, spec);
        list.reveal(PHONE, &geo, 99);
        list.update(PHONE, &ScrollInput::none(), 200, spec, 0.0);
        let r = list.row(PHONE, &geo, 99);
        assert!(r.y >= PHONE.y && r.bottom() <= PHONE.bottom(), "{r:?}");
    }
}
