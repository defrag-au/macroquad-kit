//! What a surface drawn over text does to it: the one rule the text report and
//! the DOM sink both apply, so a capture's report and a browser's spans agree
//! about what a reader can see.
//!
//! A cover is drawn after the text it covers, in draw order: a sheet's scrim
//! and card, a toast, a wallet modal, a page's fixed tape. On a canvas it
//! simply paints over what is there. Text the browser draws (the DOM sink) and
//! text a report records never get painted over, so they are told instead:
//!
//! - **An opaque cover** hides a run it wholly covers, and clips one it partly
//!   covers to the largest strip still showing.
//! - **A translucent cover** (a scrim) dims a run mostly under it by its
//!   opacity.

use macroquad::prelude::Rect;

/// At or above this opacity a cover hides what is under it.
pub const OPAQUE: f32 = 0.98;

/// What one cover does to one run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Effect {
    /// Nothing: the cover misses it.
    Clear,
    /// Wholly covered: not seen.
    Hidden,
    /// Partly covered by an opaque surface: only this part, inside the run's
    /// visible box, still shows.
    Clip(Rect),
    /// Under a translucent cover: its alpha is multiplied by this.
    Dim(f32),
}

/// What `cover`, at `opacity`, does to a run whose box is `run` and whose
/// clip (if any) is `clip`.
pub fn effect(run: Rect, clip: Option<Rect>, cover: Rect, opacity: f32) -> Effect {
    let visible = match clip {
        Some(c) => match run.intersect(c) {
            Some(v) if v.w > 0.0 && v.h > 0.0 => v,
            _ => return Effect::Clear,
        },
        None => run,
    };
    let inter = match visible.intersect(cover) {
        Some(i) if i.w > 0.0 && i.h > 0.0 => i,
        _ => return Effect::Clear,
    };
    if opacity < OPAQUE {
        // A scrim dims what is mostly under it; a run only grazing its edge
        // (a card's soft fade, a toast's corner) reads as it did.
        return match inter.w * inter.h > 0.5 * visible.w * visible.h {
            true => Effect::Dim((1.0 - opacity).clamp(0.0, 1.0)),
            false => Effect::Clear,
        };
    }
    // The four strips of the visible box outside the cover; the biggest shows.
    let strips = [
        Rect::new(visible.x, visible.y, cover.x - visible.x, visible.h),
        Rect::new(
            cover.right(),
            visible.y,
            visible.right() - cover.right(),
            visible.h,
        ),
        Rect::new(visible.x, visible.y, visible.w, cover.y - visible.y),
        Rect::new(
            visible.x,
            cover.bottom(),
            visible.w,
            visible.bottom() - cover.bottom(),
        ),
    ];
    // Relative to the run, not in any unit: the report works in points and the
    // DOM sink in fractions of the canvas, and a sliver is a sliver in both.
    let sliver = visible.w * visible.h * 0.01;
    match strips
        .into_iter()
        .filter(|s| s.w > 0.0 && s.h > 0.0 && s.w * s.h > sliver)
        .max_by(|a, b| (a.w * a.h).total_cmp(&(b.w * b.h)))
    {
        Some(strip) => Effect::Clip(strip),
        None => Effect::Hidden,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUN: Rect = Rect {
        x: 10.0,
        y: 100.0,
        w: 200.0,
        h: 20.0,
    };

    #[test]
    fn an_opaque_card_over_a_run_hides_it() {
        let card = Rect::new(0.0, 50.0, 400.0, 200.0);
        assert_eq!(effect(RUN, None, card, 1.0), Effect::Hidden);
    }

    #[test]
    fn an_opaque_surface_over_part_of_a_run_leaves_the_largest_strip() {
        // A tape over the run's top 6 points: the 14 below still show.
        let tape = Rect::new(0.0, 0.0, 400.0, 106.0);
        assert_eq!(
            effect(RUN, None, tape, 1.0),
            Effect::Clip(Rect::new(10.0, 106.0, 200.0, 14.0))
        );
        // A toast over the run's right half: the left half shows.
        let toast = Rect::new(110.0, 90.0, 300.0, 40.0);
        assert_eq!(
            effect(RUN, None, toast, 1.0),
            Effect::Clip(Rect::new(10.0, 100.0, 100.0, 20.0))
        );
    }

    #[test]
    fn the_rule_is_the_same_in_canvas_fractions() {
        // The DOM sink's units: a line 0.02 of the canvas tall, crossing a card's left edge.
        let run = Rect::new(0.1, 0.5, 0.3, 0.02);
        let card = Rect::new(0.26, 0.05, 0.47, 0.9);
        let Effect::Clip(strip) = effect(run, None, card, 1.0) else {
            panic!("the part left of the card shows");
        };
        assert!((strip.x - 0.1).abs() < 1e-6 && (strip.w - 0.16).abs() < 1e-6);
        assert!((strip.y - 0.5).abs() < 1e-6 && (strip.h - 0.02).abs() < 1e-6);
    }

    #[test]
    fn a_scrim_dims_what_is_mostly_under_it() {
        let scrim = Rect::new(0.0, 0.0, 400.0, 800.0);
        assert_eq!(effect(RUN, None, scrim, 0.74), Effect::Dim(1.0 - 0.74));
        // A run only grazing it is left alone.
        let edge = Rect::new(0.0, 115.0, 400.0, 800.0);
        assert_eq!(effect(RUN, None, edge, 0.74), Effect::Clear);
    }

    #[test]
    fn a_cover_outside_a_run_or_its_clip_does_nothing() {
        let away = Rect::new(0.0, 300.0, 400.0, 100.0);
        assert_eq!(effect(RUN, None, away, 1.0), Effect::Clear);
        // The run's clip already hides the part the cover would have covered.
        let clip = Some(Rect::new(0.0, 0.0, 100.0, 800.0));
        let right = Rect::new(150.0, 0.0, 300.0, 800.0);
        assert_eq!(effect(RUN, clip, right, 1.0), Effect::Clear);
    }
}
