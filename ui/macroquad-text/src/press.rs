//! Naming the frame's presses by the text drawn on them.
//!
//! A canvas button is a rectangle a widget hit-tests and some text drawn over
//! it; nothing ties the two together. A screen reader, a keyboard and (one day)
//! a platform's accessibility tree all need the pair: *a button, called
//! "Connect Rabby", here*. Every press a widget offers already passes through
//! `Painter::tapped` or `Painter::interact`, which report its rectangle
//! ([`crate::dom::press`]); every word drawn on it already passes through the
//! text sink. This joins them, once a frame, with one rule:
//!
//! - A run belongs to the **smallest** press whose box holds the run's centre:
//!   a button's label is the button's, not the card's it sits in.
//! - A press's name is its runs' text, in draw order, joined by a space.
//! - A press with no text on it (an icon, a tap-to-dismiss area) has no name,
//!   and is left out: an unnamed button is noise to a screen reader.
//! - The same box offered twice in a frame (a widget asking `tapped` and then
//!   `interact`) is one press.
//!
//! Pure: boxes in any one unit, no macroquad state, so it is tested natively
//! though only the web sink uses it today.

use macroquad::prelude::{Rect, vec2};

/// What [`name_presses`] made of a frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Named {
    /// Each named press, in the order first offered: its box and its name.
    pub presses: Vec<(Rect, String)>,
    /// For each run, whether it is part of a press's name: the page hides it
    /// from a screen reader, which hears it as the button instead.
    pub claimed: Vec<bool>,
}

/// Name `presses` by the `runs` drawn on them (each run's visible box and its
/// text). See the [module docs](self).
pub fn name_presses(presses: &[Rect], runs: &[(Rect, &str)]) -> Named {
    let mut unique: Vec<Rect> = Vec::with_capacity(presses.len());
    for p in presses {
        if p.w > 0.0 && p.h > 0.0 && !unique.iter().any(|u| same(*u, *p)) {
            unique.push(*p);
        }
    }
    let mut names: Vec<Vec<&str>> = vec![Vec::new(); unique.len()];
    let mut claimed = vec![false; runs.len()];
    for (i, (rect, text)) in runs.iter().enumerate() {
        if text.trim().is_empty() {
            continue;
        }
        let centre = vec2(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
        let owner = unique
            .iter()
            .enumerate()
            .filter(|(_, p)| p.contains(centre))
            .min_by(|(_, a), (_, b)| (a.w * a.h).total_cmp(&(b.w * b.h)))
            .map(|(j, _)| j);
        if let Some(j) = owner {
            let word = spoken(text);
            if !word.is_empty() {
                names[j].push(word);
            }
            claimed[i] = true;
        }
    }
    let presses = unique
        .into_iter()
        .zip(names)
        .filter(|(_, words)| !words.is_empty())
        .map(|(rect, words)| (rect, words.join(" ")))
        .collect();
    Named { presses, claimed }
}

/// A run as a name says it: without the prompt marks and bullets a terminal
/// face draws around a label (`> START`, `· 3 listed`, `→`), which a screen
/// reader would read out as "greater than".
fn spoken(text: &str) -> &str {
    const DECOR: &[char] = &[
        '>', '<', '·', '•', '›', '‹', '»', '«', '→', '←', '|', '-', '_',
    ];
    text.trim()
        .trim_matches(|c: char| c.is_whitespace() || DECOR.contains(&c))
}

/// Whether a real element the page placed already covers `press`: one of
/// `handled` overlaps most of it. A field's input is laid over the field's box
/// and a page button over its press, so the overlap is near total; a press
/// merely beside one shares little.
pub fn is_handled(press: Rect, handled: &[Rect]) -> bool {
    let area = press.w * press.h;
    area > 0.0
        && handled
            .iter()
            .any(|h| press.intersect(*h).is_some_and(|i| i.w * i.h > 0.5 * area))
}

/// Two boxes within a hair of each other are the same press: one widget's
/// rectangle, asked about twice, can differ in the last float bit.
fn same(a: Rect, b: Rect) -> bool {
    let eps = 1e-4 * (a.w.abs() + a.h.abs()).max(1e-6);
    (a.x - b.x).abs() < eps
        && (a.y - b.y).abs() < eps
        && (a.w - b.w).abs() < eps
        && (a.h - b.h).abs() < eps
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::new(x, y, w, h)
    }

    #[test]
    fn a_button_is_named_by_the_text_drawn_on_it() {
        let named = name_presses(
            &[r(0.0, 0.0, 100.0, 40.0)],
            &[(r(10.0, 10.0, 60.0, 20.0), "CONNECT RABBY")],
        );
        assert_eq!(
            named.presses,
            [(r(0.0, 0.0, 100.0, 40.0), "CONNECT RABBY".to_string())]
        );
        assert_eq!(named.claimed, [true]);
    }

    #[test]
    fn text_beside_a_button_is_not_its_name() {
        let named = name_presses(
            &[r(0.0, 0.0, 100.0, 40.0)],
            &[
                (r(10.0, 10.0, 60.0, 20.0), "COPY"),
                (r(10.0, 60.0, 60.0, 20.0), "Your code is below"),
            ],
        );
        assert_eq!(named.presses[0].1, "COPY");
        assert_eq!(named.claimed, [true, false]);
    }

    #[test]
    fn several_runs_name_a_press_in_draw_order() {
        let named = name_presses(
            &[r(0.0, 0.0, 300.0, 90.0)],
            &[
                (r(10.0, 10.0, 100.0, 14.0), "CLONING CHAMBER"),
                (r(10.0, 30.0, 100.0, 20.0), "0x12…ab"),
                (r(10.0, 55.0, 100.0, 16.0), "wallet not connected · tap"),
            ],
        );
        assert_eq!(
            named.presses[0].1,
            "CLONING CHAMBER 0x12…ab wallet not connected · tap"
        );
    }

    #[test]
    fn a_button_inside_a_card_keeps_its_own_label() {
        let card = r(0.0, 0.0, 300.0, 100.0);
        let unlink = r(220.0, 30.0, 70.0, 40.0);
        let named = name_presses(
            &[card, unlink],
            &[
                (r(10.0, 40.0, 100.0, 20.0), "CELL LINE 01"),
                (r(230.0, 40.0, 50.0, 20.0), "UNLINK"),
            ],
        );
        assert_eq!(
            named.presses,
            [
                (card, "CELL LINE 01".to_string()),
                (unlink, "UNLINK".to_string())
            ]
        );
    }

    #[test]
    fn a_press_with_no_text_is_left_out() {
        let named = name_presses(
            &[r(0.0, 0.0, 40.0, 40.0)],
            &[(r(100.0, 0.0, 40.0, 20.0), "elsewhere")],
        );
        assert!(named.presses.is_empty());
        assert_eq!(named.claimed, [false]);
    }

    #[test]
    fn the_same_box_offered_twice_is_one_press() {
        let b = r(0.0, 0.0, 100.0, 40.0);
        let mut nudged = b;
        nudged.x += 1e-7;
        let named = name_presses(&[b, nudged], &[(r(10.0, 10.0, 60.0, 20.0), "OK")]);
        assert_eq!(named.presses.len(), 1);
    }

    #[test]
    fn prompt_marks_and_bullets_are_not_spoken() {
        let named = name_presses(
            &[r(0.0, 0.0, 300.0, 40.0)],
            &[
                (r(10.0, 10.0, 200.0, 20.0), "> START IN ROBINHOOD WALLET"),
                (r(220.0, 10.0, 40.0, 20.0), "STEP 1"),
                (r(260.0, 10.0, 10.0, 20.0), "→"),
            ],
        );
        assert_eq!(named.presses[0].1, "START IN ROBINHOOD WALLET STEP 1");
        // A mark-only run is still part of the button, so not read on its own.
        assert_eq!(named.claimed, [true, true, true]);
        // Inside a label, marks stay: "$PERP · 3 listed" is said as written.
        assert_eq!(spoken("· 3 $PERP · held"), "3 $PERP · held");
    }

    #[test]
    fn a_press_under_a_real_element_is_the_elements() {
        let copy = r(0.0, 0.0, 100.0, 36.0);
        assert!(is_handled(copy, &[r(0.0, 0.0, 100.0, 36.0)]));
        // A neighbour sharing an edge, or a sliver, is not.
        assert!(!is_handled(copy, &[r(100.0, 0.0, 100.0, 36.0)]));
        assert!(!is_handled(copy, &[r(90.0, 0.0, 100.0, 36.0)]));
        assert!(!is_handled(copy, &[]));
    }

    #[test]
    fn blank_runs_and_empty_boxes_name_nothing() {
        let named = name_presses(
            &[r(0.0, 0.0, 0.0, 40.0), r(0.0, 0.0, 100.0, 40.0)],
            &[(r(10.0, 10.0, 60.0, 20.0), "   ")],
        );
        assert!(named.presses.is_empty());
        assert_eq!(named.claimed, [false]);
    }
}
