//! Buttons whose action a browser allows only inside the tap's own handler.
//!
//! A browser writes the clipboard (and opens a tab, and hands a link to an
//! app) only while it handles the reader's gesture. How long that lasts
//! differs: Chrome allows the next frame, WebKit and some phone wallets'
//! in-app browsers allow only the handler itself. A canvas acts on a tap in
//! its next frame, so a press drawn on the canvas cannot do these things
//! everywhere.
//!
//! A page button closes the gap. The widget draws its press as any other, and
//! the page lays a real, invisible `<button>` over it: the tap lands on the
//! element, the browser decides it was pressed, and the action runs inside its
//! click handler. The widget reads back what the browser did, so it says
//! "copied" only when the browser copied. Natively there is no page: the
//! canvas's own press does the action through miniquad.
//!
//! The app never names a box or an id: a widget owns both, the way a
//! [`TextField`](crate::TextField) owns its field. Like a field, it is drawn
//! between [`Keyboard::begin`] and [`Keyboard::end`], the frame of everything
//! the page lays over the canvas, and a frame that does not draw it removes
//! its element.
//!
//! [`CopyButton`] is the first; opening a link from a real `<a>` is the
//! natural second.

use std::sync::atomic::{AtomicU32, Ordering};

use macroquad::prelude::Rect;
use macroquad_widgets::{Button, ButtonVariant, Painter};

use crate::Keyboard;
use crate::input::{PageAction, PageReply};

/// How long a copy button says what became of a copy, in seconds.
pub const SAID_SECS: f64 = 2.0;

static NEXT_ID: AtomicU32 = AtomicU32::new(1);

/// A fresh page-button id: unique for the life of the page, so two widgets
/// never share an element.
pub(crate) fn next_id() -> u32 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// What the browser did with a copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyOutcome {
    /// The text is on the clipboard.
    Copied,
    /// The browser refused the clipboard. Say so, and keep the text on
    /// screen to copy by hand.
    Refused,
}

/// A press that copies its text to the clipboard. See the [module docs](self).
///
/// ```ignore
/// // once
/// let mut copy = CopyButton::new("COPY CODE");
/// // every frame the code is shown, between keyboard.begin() and .end()
/// if copy.show(p, &mut keyboard, rect, &code, now) == Some(CopyOutcome::Refused) {
///     // the code stays on screen to copy by hand
/// }
/// ```
#[derive(Clone)]
pub struct CopyButton {
    id: u32,
    label: String,
    copied: String,
    refused: String,
    variant: ButtonVariant,
    said: Option<(f64, CopyOutcome)>,
}

impl CopyButton {
    /// A copy press reading `label`; "Copied" and "Copy blocked" after a copy.
    pub fn new(label: &str) -> Self {
        Self {
            id: next_id(),
            label: label.to_owned(),
            copied: "Copied".to_owned(),
            refused: "Copy blocked".to_owned(),
            variant: ButtonVariant::Filled,
            said: None,
        }
    }

    /// What the press reads for [`SAID_SECS`] after the browser copied, and
    /// after it refused.
    pub fn labels(mut self, copied: &str, refused: &str) -> Self {
        self.copied = copied.to_owned();
        self.refused = refused.to_owned();
        self
    }

    /// The face [`CopyButton::show`] draws.
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    /// The outcome the press is saying at `now`, if it is still saying one.
    pub fn said(&self, now: f64) -> Option<CopyOutcome> {
        self.said
            .filter(|(at, _)| now >= *at && now - *at < SAID_SECS)
            .map(|(_, outcome)| outcome)
    }

    /// The label at `now`: the press's own, or what became of the last copy.
    pub fn face(&self, now: f64) -> &str {
        match self.said(now) {
            Some(CopyOutcome::Copied) => &self.copied,
            Some(CopyOutcome::Refused) => &self.refused,
            None => &self.label,
        }
    }

    /// Record an outcome at `now`, as a copy would: what a capture uses to
    /// show the press after a copy.
    pub fn note(&mut self, outcome: CopyOutcome, now: f64) {
        self.said = Some((now, outcome));
    }

    /// Draw the press in its own face over `rect`, copying `text`. `now` is
    /// the app's clock in seconds, for how long the press says "copied".
    /// Returns what the browser did this frame, if it did anything.
    pub fn show(
        &mut self,
        p: &Painter,
        keyboard: &mut Keyboard,
        rect: Rect,
        text: &str,
        now: f64,
    ) -> Option<CopyOutcome> {
        let variant = self.variant;
        self.show_with(p, keyboard, rect, text, now, |p, rect, label| {
            Button::new(label).variant(variant).show(p, rect)
        })
    }

    /// [`CopyButton::show`] with the app's own face: `draw` paints the press
    /// with the label it is given and returns whether the canvas saw it
    /// pressed. On the web the page's element takes the tap, so that is only
    /// ever true natively.
    pub fn show_with(
        &mut self,
        p: &Painter,
        keyboard: &mut Keyboard,
        rect: Rect,
        text: &str,
        now: f64,
        draw: impl FnOnce(&Painter, Rect, &str) -> bool,
    ) -> Option<CopyOutcome> {
        let clicked = draw(p, rect, self.face(now));
        let reply = keyboard.page_button(self.id, rect, PageAction::Copy(text), clicked);
        let outcome = outcome(reply);
        if let Some(outcome) = outcome {
            self.note(outcome, now);
        }
        outcome
    }
}

/// A page reply as a copy's outcome. A frame that saw both (two taps between
/// frames, one taken and one refused) reports the refusal: the reader should
/// not be told it worked when the last try did not.
fn outcome(reply: PageReply) -> Option<CopyOutcome> {
    match (reply.done, reply.refused) {
        (_, true) => Some(CopyOutcome::Refused),
        (true, false) => Some(CopyOutcome::Copied),
        (false, false) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_button_has_its_own_id() {
        let a = CopyButton::new("A");
        let b = CopyButton::new("B");
        let c = a.clone();
        assert_ne!(a.id, b.id);
        // A clone is the same button: it names the same element.
        assert_eq!(a.id, c.id);
    }

    #[test]
    fn the_press_reads_its_label_until_a_copy() {
        let copy = CopyButton::new("COPY CODE");
        assert_eq!(copy.face(0.0), "COPY CODE");
        assert_eq!(copy.said(0.0), None);
    }

    #[test]
    fn a_copy_is_said_for_two_seconds_then_the_label_returns() {
        let mut copy = CopyButton::new("COPY CODE").labels("COPIED", "BLOCKED");
        copy.note(CopyOutcome::Copied, 10.0);
        assert_eq!(copy.face(10.0), "COPIED");
        assert_eq!(copy.face(10.0 + SAID_SECS - 0.01), "COPIED");
        assert_eq!(copy.face(10.0 + SAID_SECS), "COPY CODE");
        // A clock read before the copy (a capture's pinned clock) says nothing.
        assert_eq!(copy.face(9.0), "COPY CODE");
    }

    #[test]
    fn a_refusal_is_said_as_one() {
        let mut copy = CopyButton::new("Copy");
        copy.note(CopyOutcome::Refused, 0.0);
        assert_eq!(copy.face(0.5), "Copy blocked");
        assert_eq!(copy.said(0.5), Some(CopyOutcome::Refused));
    }

    #[test]
    fn the_latest_outcome_replaces_the_last() {
        let mut copy = CopyButton::new("Copy");
        copy.note(CopyOutcome::Refused, 0.0);
        copy.note(CopyOutcome::Copied, 1.0);
        assert_eq!(copy.said(1.5), Some(CopyOutcome::Copied));
    }

    #[test]
    fn a_reply_becomes_an_outcome_and_a_refusal_wins_a_tie() {
        let reply = |done, refused| PageReply { done, refused };
        assert_eq!(outcome(reply(false, false)), None);
        assert_eq!(outcome(reply(true, false)), Some(CopyOutcome::Copied));
        assert_eq!(outcome(reply(false, true)), Some(CopyOutcome::Refused));
        assert_eq!(outcome(reply(true, true)), Some(CopyOutcome::Refused));
    }

    #[test]
    fn the_page_script_knows_the_copy_kind() {
        assert_eq!(PageAction::Copy("x").wire(), (1, "x"));
        let js = crate::PLUGIN_JS;
        assert!(js.contains("const BUTTON_COPY = 1;"));
        for name in [
            "app_buttons_clear",
            "app_button",
            "app_buttons_done",
            "app_button_done",
            "app_button_refused",
        ] {
            assert!(
                js.contains(&format!("importObject.env.{name} = {name};")),
                "{name} is not registered"
            );
        }
    }
}
