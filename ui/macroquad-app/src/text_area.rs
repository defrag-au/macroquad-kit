//! `TextArea` — a multi-line field that grows with what it holds and scrolls
//! inside once it reaches a cap, with the phone's own keyboard.
//!
//! The editing model is here rather than in the browser, so a capture, a test
//! and a browser edit the same text. [`TextArea`] holds the value, the caret
//! and the selection's anchor; [`TextArea::show`] lays the text out at the
//! width it is given, draws it, and applies this frame's keys.
//!
//! On the web the canvas cannot take text, so while the area is edited a real
//! `<textarea>` sits over it (the app shell's page script), bringing the
//! phone's autocorrect, paste and IME; the page owns the text for those frames
//! and this reads it back, caret and internal scroll included. Natively the
//! area reads [`Keyboard`], which carries the arrows, Home and End, Delete,
//! shift-selection, select-all, copy, cut and paste beside the typing.
//!
//! ```ignore
//! let reply = self.body.show(
//!     &p,
//!     &mut self.keyboard,
//!     Rect::new(lx, y, w, 0.0),
//!     TextAreaSpec::new(FieldKind::Text, "Write a note").max_h(320.0),
//!     &input,
//!     frame.dt,
//! );
//! y += self.body.height() + 16.0;
//! if reply.editing {
//!     page.reveal(body, self.body.caret_rect(), insets.bottom_cover());
//! }
//! ```
//!
//! Return is a newline, as it is in a phone's notes app; the submit is
//! Ctrl/Cmd+Enter. `rect`'s height is not used: the area is
//! [`TextAreaSpec::min_h`], grown to its text, up to [`TextAreaSpec::max_h`],
//! and [`TextArea::height`] is what the next thing on the page is placed after.

use macroquad::prelude::*;
use macroquad_gamekit::clip::with_clip;
use macroquad_widgets::{
    Painter, Scroll, ScrollInput, fill_rounded, stroke_rounded, theme::with_alpha,
};
use ui_theme::TextSize;

#[cfg(target_arch = "wasm32")]
use crate::input::page;
use crate::input::{FieldKind, Keyboard};

/// The area's least height: three lines and the padding.
pub const TEXT_AREA_MIN_H: f32 = 96.0;

/// The height it grows to before it scrolls inside.
pub const TEXT_AREA_MAX_H: f32 = 320.0;

const PAD_X: f32 = 14.0;
const PAD_Y: f32 = 12.0;
const LINE_SPACING: f32 = 1.45;

/// The area's shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextAreaSpec<'a> {
    /// What it holds, which sets the phone's keyboard up for it.
    pub kind: FieldKind,
    pub placeholder: &'a str,
    /// Its height when empty, and its floor as it grows.
    pub min_h: f32,
    /// The height it grows to before it scrolls inside.
    pub max_h: f32,
    /// The most characters it takes, or `None`. Counted in characters, not
    /// bytes, so an emoji is one.
    pub max_len: Option<usize>,
}

impl<'a> TextAreaSpec<'a> {
    pub fn new(kind: FieldKind, placeholder: &'a str) -> Self {
        Self {
            kind,
            placeholder,
            min_h: TEXT_AREA_MIN_H,
            max_h: TEXT_AREA_MAX_H,
            max_len: None,
        }
    }

    pub fn min_h(mut self, height: f32) -> Self {
        self.min_h = height;
        self
    }

    pub fn max_h(mut self, height: f32) -> Self {
        self.max_h = height;
        self
    }

    pub fn max_len(mut self, chars: usize) -> Self {
        self.max_len = Some(chars);
        self
    }
}

/// What happened to a text area this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextAreaReply {
    /// The text is different from last frame's.
    pub changed: bool,
    /// The reader pressed Ctrl/Cmd+Enter.
    pub submitted: bool,
    /// The area is being edited.
    pub editing: bool,
}

/// One visual line: a wrapped or hard-broken piece of the value.
#[derive(Clone, Copy, Debug, PartialEq)]
struct VisualLine {
    /// First byte drawn on the line.
    start: usize,
    /// One past the last byte drawn. A line wrapped at a space leaves the
    /// space out, as an editor does.
    end: usize,
    /// One past the last byte the line owns for a caret: it takes in the
    /// wrapped space, or the hard newline, that ends it.
    next: usize,
}

/// See the [module docs](self).
#[derive(Clone, Debug, Default)]
pub struct TextArea {
    id: u32,
    value: String,
    /// The caret, as a byte offset at a character boundary.
    caret: usize,
    /// The selection's other end, if a selection is being made.
    anchor: Option<usize>,
    scroll: Scroll,
    /// The value laid out for the width it was last drawn at.
    lines: Vec<VisualLine>,
    height: f32,
    caret_rect: Rect,
    max_len: Option<usize>,
    /// The page's `<textarea>` scroll this frame, in CSS pixels.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    page_scroll: f32,
}

impl TextArea {
    /// An area known to the page as `id`: unique among the fields on screen at
    /// once.
    pub fn new(id: u32) -> Self {
        Self {
            id,
            ..Self::default()
        }
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    /// Replace the text, including under the reader's fingers if the area is
    /// being edited, and put the caret at its end.
    ///
    /// Authoritative: [`TextAreaSpec::max_len`] is not applied, because a value
    /// restored from a route or a server is not typing.
    pub fn set(&mut self, keyboard: &Keyboard, value: &str) {
        self.value.clear();
        self.value.push_str(value);
        self.caret = self.value.len();
        self.anchor = None;
        #[cfg(target_arch = "wasm32")]
        if keyboard.focused == Some(self.id) {
            page::set(value);
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = keyboard;
    }

    /// The height the last [`TextArea::show`] used, for the next thing down the
    /// page.
    pub fn height(&self) -> f32 {
        self.height
    }

    /// The caret's box from the last [`TextArea::show`], in screen units: what
    /// [`crate::Page::reveal`] keeps above the keyboard.
    pub fn caret_rect(&self) -> Rect {
        self.caret_rect
    }

    /// Draw the area at `rect`'s x and width, taking this frame's typing.
    ///
    /// `rect`'s height is ignored; see [`TextArea::height`]. `input` is the
    /// frame's scroll, for the area's own scroll once its text outgrows
    /// [`TextAreaSpec::max_h`].
    pub fn show(
        &mut self,
        p: &Painter,
        keyboard: &mut Keyboard,
        rect: Rect,
        spec: TextAreaSpec,
        input: &ScrollInput,
        dt: f32,
    ) -> TextAreaReply {
        let c = &p.theme.color;
        let size = p.size(TextSize::Lg);
        let line_h = size * LINE_SPACING;
        let measure = |s: &str| p.measure(s, size).width;
        let text_w = (rect.w - PAD_X * 2.0).max(1.0);
        let before = self.value.clone();
        self.max_len = spec.max_len;

        // The tap: focus inside, dismiss outside, as a single-line field does.
        let mut reply = TextAreaReply::default();
        if let Some(tap) = p.tap {
            let inside = rect.contains(tap);
            if inside && keyboard.focused != Some(self.id) {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    keyboard.focused = Some(self.id);
                }
            } else if !inside && keyboard.focused == Some(self.id) {
                keyboard.dismiss();
            }
        }

        // The keys. A text area moves the caret, so the arrows need the layout
        // the value had when the frame began; typing then changes it.
        if keyboard.focused == Some(self.id) {
            #[cfg(target_arch = "wasm32")]
            {
                self.read_page(keyboard);
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                let lines = wrap(&self.value, text_w, measure);
                let submit = keyboard.typed.submit;
                if self.apply_keys(&keyboard.typed, &lines, &measure) {
                    keyboard.focused = None;
                }
                reply.submitted = submit;
            }
        }
        // The web's submit is the page's; Enter there is a newline in the DOM.
        #[cfg(target_arch = "wasm32")]
        if keyboard.submitted && keyboard.was_focused == Some(self.id) {
            reply.submitted = true;
        }

        let editing = keyboard.focused == Some(self.id);
        reply.editing = editing;
        reply.changed = self.value != before;

        self.lines = wrap(&self.value, text_w, measure);
        let content_h = self.lines.len() as f32 * line_h + PAD_Y * 2.0;
        let min_h = spec.min_h.max(line_h + PAD_Y * 2.0);
        let max_h = spec.max_h.max(min_h);
        // Once there is more text than fits, the box holds a whole number of
        // lines: a caret-follow scroll lands on a line's top rather than
        // cutting the first visible line in half.
        let height = if content_h > max_h {
            let capacity = ((max_h - PAD_Y * 2.0) / line_h).floor().max(1.0);
            (capacity * line_h + PAD_Y * 2.0).clamp(min_h, max_h)
        } else {
            content_h.clamp(min_h, max_h)
        };
        let widget = Rect::new(rect.x, rect.y, rect.w, height);
        let inner = Rect::new(
            widget.x + PAD_X,
            widget.y + PAD_Y,
            text_w,
            (height - PAD_Y * 2.0).max(0.0),
        );
        self.height = height;

        // While the page's textarea is edited it scrolls itself; the canvas
        // follows it, and takes no wheel or drag of its own.
        #[cfg(target_arch = "wasm32")]
        let scroll_input = if editing { ScrollInput::none() } else { *input };
        #[cfg(not(target_arch = "wasm32"))]
        let scroll_input = *input;
        let max_scroll = (content_h - height).max(0.0);
        self.scroll
            .update(widget, &scroll_input, Some(max_scroll), dt);
        #[cfg(target_arch = "wasm32")]
        if editing {
            self.scroll.set(self.page_scroll / keyboard.unit());
        }
        #[cfg(not(target_arch = "wasm32"))]
        if editing {
            self.keep_caret_in_view(line_h, height);
        }

        let off = self.scroll.offset();
        let line = self.line_of(self.caret);
        let caret_x = inner.x + self.x_at(&self.lines[line], self.caret, measure);
        let caret_y = inner.y + line as f32 * line_h - off;
        let caret_h = size * 1.2;
        self.caret_rect = Rect::new(caret_x.min(inner.right() - 1.5), caret_y, 1.5, caret_h);

        #[cfg(target_arch = "wasm32")]
        page::zone(page::Zone {
            id: self.id,
            frac: keyboard.fractions(inner),
            value: &self.value,
            placeholder: spec.placeholder,
            kind: spec.kind.code(),
            font: size * keyboard.unit(),
            color: &crate::css_rgba(c.text_primary),
            multiline: true,
            max_len: u32::try_from(spec.max_len.unwrap_or(0)).unwrap_or(u32::MAX),
        });

        fill_rounded(widget, 12.0, c.bg_secondary);
        if editing {
            stroke_rounded(widget, 12.0, 1.5, c.accent);
        } else {
            stroke_rounded(widget, 12.0, 1.0, c.border);
        }

        let page_draws = cfg!(target_arch = "wasm32") && editing;
        if self.value.is_empty() && !editing {
            let hint = p.fit(spec.placeholder, size, inner.w);
            p.text_top(&hint, inner.x, inner.y, size, c.text_muted);
        }
        if !page_draws && !self.value.is_empty() {
            with_clip(inner, || {
                if let Some((sel_a, sel_b)) = self.selection() {
                    for (i, line) in self.lines.iter().enumerate() {
                        if sel_b <= line.start || sel_a >= line.next {
                            continue;
                        }
                        let from = sel_a.clamp(line.start, line.end);
                        let to = sel_b.clamp(line.start, line.end);
                        let x0 = inner.x + self.x_at(line, from, measure);
                        let x1 = inner.x + self.x_at(line, to, measure);
                        let top = inner.y + i as f32 * line_h - off;
                        draw_rectangle(
                            x0,
                            top,
                            (x1 - x0).max(1.0),
                            line_h,
                            with_alpha(c.accent, 0.22),
                        );
                    }
                }
                for (i, line) in self.lines.iter().enumerate() {
                    let top = inner.y + i as f32 * line_h - off;
                    if top >= inner.bottom() || top + line_h <= inner.y {
                        continue;
                    }
                    p.text_top(
                        &self.value[line.start..line.end],
                        inner.x,
                        top,
                        size,
                        c.text_primary,
                    );
                }
                // The caret blinks once a second, on the host's clock.
                if editing && p.time().fract() < 0.5 {
                    draw_rectangle(
                        self.caret_rect.x,
                        caret_y + (line_h - caret_h) * 0.5,
                        self.caret_rect.w,
                        caret_h,
                        c.accent,
                    );
                }
            });
        }
        reply
    }

    /// Read the page's `<textarea>` back into the model while it owns the text.
    #[cfg(target_arch = "wasm32")]
    fn read_page(&mut self, keyboard: &Keyboard) {
        if keyboard.focused != Some(self.id) {
            return;
        }
        self.value = page::value();
        let caret = page::caret();
        let anchor = page::anchor();
        let at = |units: f32| {
            if units < 0.0 {
                self.value.len()
            } else {
                byte_at_utf16(&self.value, units as usize)
            }
        };
        self.caret = at(caret);
        self.anchor = if anchor < 0.0 {
            None
        } else {
            let anchor = at(anchor);
            (anchor != self.caret).then_some(anchor)
        };
        self.page_scroll = page::scroll_top();
    }

    /// Apply this frame's native keys. Returns whether the area should give up
    /// the focus (Escape, or Ctrl/Cmd+Enter).
    #[cfg(not(target_arch = "wasm32"))]
    fn apply_keys(
        &mut self,
        typed: &crate::input::Typed,
        lines: &[VisualLine],
        measure: &impl Fn(&str) -> f32,
    ) -> bool {
        if typed.escape {
            return true;
        }
        if typed.select_all {
            self.anchor = Some(0);
            self.caret = self.value.len();
        }
        if typed.copy {
            self.copy();
        }
        if typed.cut {
            self.cut();
        }
        if let Some(paste) = &typed.paste {
            self.insert(paste);
        }
        if typed.left {
            self.move_horizontal(true, typed.shift);
        }
        if typed.right {
            self.move_horizontal(false, typed.shift);
        }
        if typed.up {
            self.move_vertical(-1, typed.shift, lines, measure);
        }
        if typed.down {
            self.move_vertical(1, typed.shift, lines, measure);
        }
        if typed.home {
            self.move_to_line_edge(true, typed.shift, lines);
        }
        if typed.end {
            self.move_to_line_edge(false, typed.shift, lines);
        }
        for _ in 0..typed.backspaces {
            self.backspace();
        }
        if typed.delete {
            self.delete_forward();
        }
        if !typed.text.is_empty() {
            self.insert(&typed.text);
        }
        if typed.enter {
            self.insert("\n");
        }
        typed.submit
    }

    fn selection(&self) -> Option<(usize, usize)> {
        let anchor = self.anchor?;
        if anchor == self.caret {
            return None;
        }
        Some(if anchor < self.caret {
            (anchor, self.caret)
        } else {
            (self.caret, anchor)
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn delete_selection(&mut self) -> bool {
        let Some((start, end)) = self.selection() else {
            return false;
        };
        self.value.replace_range(start..end, "");
        self.caret = start;
        self.anchor = None;
        true
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn insert(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.delete_selection();
        let text = match self.max_len {
            Some(max) => {
                let room = max.saturating_sub(self.value.chars().count());
                text.chars().take(room).collect::<String>()
            }
            None => text.to_owned(),
        };
        if text.is_empty() {
            return;
        }
        self.value.insert_str(self.caret, &text);
        self.caret += text.len();
        self.anchor = None;
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn backspace(&mut self) {
        if self.delete_selection() || self.caret == 0 {
            return;
        }
        let from = prev_boundary(&self.value, self.caret);
        self.value.replace_range(from..self.caret, "");
        self.caret = from;
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn delete_forward(&mut self) {
        if self.delete_selection() || self.caret >= self.value.len() {
            return;
        }
        let to = next_boundary(&self.value, self.caret);
        self.value.replace_range(self.caret..to, "");
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn copy(&mut self) -> bool {
        let Some((start, end)) = self.selection() else {
            return false;
        };
        macroquad::miniquad::window::clipboard_set(&self.value[start..end]);
        true
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn cut(&mut self) {
        if self.copy() {
            self.delete_selection();
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn move_horizontal(&mut self, left: bool, extend: bool) {
        if !extend && let Some((start, end)) = self.selection() {
            // A plain arrow collapses a selection to its edge, without moving
            // past it: what every editor does.
            self.caret = if left { start } else { end };
            self.anchor = None;
            return;
        }
        let to = if left {
            prev_boundary(&self.value, self.caret)
        } else {
            next_boundary(&self.value, self.caret)
        };
        if extend {
            self.anchor.get_or_insert(self.caret);
        } else {
            self.anchor = None;
        }
        self.caret = to;
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn move_vertical(
        &mut self,
        delta: isize,
        extend: bool,
        lines: &[VisualLine],
        measure: &impl Fn(&str) -> f32,
    ) {
        if lines.is_empty() {
            return;
        }
        if !extend {
            self.anchor = None;
        }
        let here = line_index(lines, self.caret);
        let x = self.x_at(&lines[here], self.caret, measure);
        let target = (here as isize + delta).clamp(0, lines.len() as isize - 1) as usize;
        let line = lines[target];
        // The caret moves to the character boundary nearest its column; the
        // line's start (an empty prefix) is the first candidate.
        let mut at = line.start;
        let mut best = x;
        for (i, ch) in self.value[line.start..line.end].char_indices() {
            let candidate = line.start + i + ch.len_utf8();
            let gap = (measure(&self.value[line.start..candidate]) - x).abs();
            if gap < best {
                best = gap;
                at = candidate;
            }
        }
        if extend {
            self.anchor.get_or_insert(self.caret);
        }
        self.caret = at;
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn move_to_line_edge(&mut self, start: bool, extend: bool, lines: &[VisualLine]) {
        if lines.is_empty() {
            return;
        }
        let line = lines[line_index(lines, self.caret)];
        if extend {
            self.anchor.get_or_insert(self.caret);
        } else {
            self.anchor = None;
        }
        self.caret = if start { line.start } else { line.end };
    }

    /// Keep the caret's line inside the area's own viewport.
    #[cfg(not(target_arch = "wasm32"))]
    fn keep_caret_in_view(&mut self, line_h: f32, height: f32) {
        let line = self.line_of(self.caret);
        let visible = (height - PAD_Y * 2.0).max(line_h);
        let top = line as f32 * line_h;
        let bottom = top + line_h;
        let off = self.scroll.offset();
        if top < off {
            self.scroll.set(top);
        } else if bottom > off + visible {
            self.scroll.set(bottom - visible);
        }
    }

    fn line_of(&self, caret: usize) -> usize {
        line_index(&self.lines, caret)
    }

    /// The width from `line`'s start to `at`, clamped to the line.
    fn x_at(&self, line: &VisualLine, at: usize, measure: impl Fn(&str) -> f32) -> f32 {
        let at = at.clamp(line.start, line.end);
        measure(&self.value[line.start..at])
    }
}

fn line_index(lines: &[VisualLine], caret: usize) -> usize {
    lines
        .iter()
        .rposition(|line| line.start <= caret)
        .unwrap_or(0)
}

/// Lay `value` out in `width`, one [`VisualLine`] per drawn line.
///
/// A hard newline starts a line; a space that overflows ends one (and is not
/// drawn, as an editor wraps it); a word wider than the width breaks between
/// characters, so nothing is ever lost. Measuring is the caller's, so the
/// layout is tested without a window.
fn wrap(value: &str, width: f32, measure: impl Fn(&str) -> f32) -> Vec<VisualLine> {
    let mut lines = Vec::new();
    let mut hard_start = 0;
    loop {
        let hard_end = value[hard_start..]
            .find('\n')
            .map(|i| hard_start + i)
            .unwrap_or(value.len());
        let has_newline = hard_end < value.len();
        let after = if has_newline { hard_end + 1 } else { hard_end };
        wrap_hard(value, hard_start, hard_end, width, &measure, &mut lines);
        if let Some(last) = lines.last_mut() {
            last.next = after;
        }
        if !has_newline {
            break;
        }
        hard_start = after;
        if hard_start == value.len() {
            // A trailing newline leaves an empty last line for the caret.
            lines.push(VisualLine {
                start: hard_start,
                end: hard_start,
                next: hard_start,
            });
            break;
        }
    }
    lines
}

fn wrap_hard(
    value: &str,
    start: usize,
    end: usize,
    width: f32,
    measure: &impl Fn(&str) -> f32,
    lines: &mut Vec<VisualLine>,
) {
    if start == end {
        lines.push(VisualLine {
            start,
            end,
            next: end,
        });
        return;
    }
    let mut line_start = start;
    while line_start < end {
        let (line_end, next) = wrap_line(value, line_start, end, width, measure);
        lines.push(VisualLine {
            start: line_start,
            end: line_end,
            next,
        });
        line_start = next;
    }
}

/// Where one line of a hard line ends, and where the next starts.
fn wrap_line(
    value: &str,
    start: usize,
    end: usize,
    width: f32,
    measure: &impl Fn(&str) -> f32,
) -> (usize, usize) {
    if measure(&value[start..end]) <= width {
        return (end, end);
    }
    // Words with the spaces after them: a line may end where a space is, and
    // the next starts past it.
    let mut scan = start;
    let mut last_break: Option<(usize, usize)> = None;
    while scan < end {
        let word_end = value[scan..end].find(' ').map(|i| scan + i).unwrap_or(end);
        if measure(&value[start..word_end]) > width {
            break;
        }
        if word_end == end {
            // The last word fits; the rest of the line would have too, and the
            // check above returned.
            scan = end;
            continue;
        }
        last_break = Some((word_end, word_end + 1));
        scan = word_end + 1;
        while scan < end && value.as_bytes()[scan] == b' ' {
            scan += 1;
        }
    }
    if let Some(split) = last_break {
        return split;
    }
    // Not even the first word fits: break after the last character that does,
    // and take one character even when none does, so the line always advances.
    let mut line_end = start;
    for (i, ch) in value[start..end].char_indices() {
        let candidate = start + i + ch.len_utf8();
        if candidate > start && measure(&value[start..candidate]) > width {
            break;
        }
        line_end = candidate;
    }
    if line_end == start {
        let ch = value[start..end].chars().next().expect("non-empty line");
        line_end = start + ch.len_utf8();
    }
    (line_end, line_end)
}

#[cfg(not(target_arch = "wasm32"))]
fn floor_boundary(value: &str, mut at: usize) -> usize {
    at = at.min(value.len());
    while !value.is_char_boundary(at) {
        at -= 1;
    }
    at
}

#[cfg(not(target_arch = "wasm32"))]
fn prev_boundary(value: &str, at: usize) -> usize {
    let mut at = floor_boundary(value, at);
    if at == 0 {
        return 0;
    }
    at -= 1;
    while !value.is_char_boundary(at) {
        at -= 1;
    }
    at
}

#[cfg(not(target_arch = "wasm32"))]
fn next_boundary(value: &str, at: usize) -> usize {
    let at = floor_boundary(value, at);
    if at >= value.len() {
        return value.len();
    }
    let mut next = at + 1;
    while next < value.len() && !value.is_char_boundary(next) {
        next += 1;
    }
    next
}

/// The byte offset of a JavaScript string index, which counts UTF-16 units.
/// An index inside a surrogate pair lands after the character.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn byte_at_utf16(value: &str, units: usize) -> usize {
    let mut seen = 0;
    for (i, ch) in value.char_indices() {
        if seen >= units {
            return i;
        }
        seen += ch.len_utf16();
    }
    value.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every character is ten units wide.
    fn mono(s: &str) -> f32 {
        s.chars().count() as f32 * 10.0
    }

    fn area(value: &str, caret: usize) -> TextArea {
        let mut out = TextArea::new(1);
        out.value = value.to_owned();
        out.caret = caret;
        out
    }

    fn laid_out(value: &str, width: f32) -> TextArea {
        let mut out = area(value, 0);
        out.lines = wrap(value, width, mono);
        out
    }

    fn line_texts(value: &str, width: f32) -> Vec<String> {
        wrap(value, width, mono)
            .into_iter()
            .map(|line| value[line.start..line.end].to_owned())
            .collect()
    }

    #[test]
    fn a_value_wraps_at_the_space_that_overflows() {
        // Ten characters fit a line: "The quick" does, "The quick brown" does not.
        assert_eq!(
            line_texts("The quick brown fox", 100.0),
            ["The quick", "brown fox"]
        );
        let lines = wrap("The quick brown fox", 100.0, mono);
        assert_eq!(
            lines[0],
            VisualLine {
                start: 0,
                end: 9,
                next: 10
            }
        );
        assert_eq!(
            lines[1],
            VisualLine {
                start: 10,
                end: 19,
                next: 19
            }
        );
    }

    #[test]
    fn a_word_wider_than_the_line_breaks_between_characters() {
        assert_eq!(line_texts("abcdefghijkl", 50.0), ["abcde", "fghij", "kl"]);
    }

    #[test]
    fn a_hard_newline_starts_a_line_and_a_trailing_one_leaves_an_empty_line() {
        assert_eq!(line_texts("ab\ncd", 1000.0), ["ab", "cd"]);
        assert_eq!(line_texts("ab\n", 1000.0), ["ab", ""]);
        assert_eq!(line_texts("\n\n", 1000.0), ["", "", ""]);
        assert_eq!(line_texts("", 1000.0), [""]);
    }

    #[test]
    fn a_caret_belongs_to_the_line_that_owns_it() {
        // "The quick" ends at 9; the space it wrapped at is byte 9, and
        // "brown" starts at 10.
        let lines = wrap("The quick brown fox", 100.0, mono);
        assert_eq!(line_index(&lines, 0), 0);
        assert_eq!(line_index(&lines, 9), 0, "the wrapped space ends its line");
        assert_eq!(line_index(&lines, 10), 1);
        assert_eq!(line_index(&lines, 19), 1);
    }

    #[test]
    fn a_caret_at_a_trailing_newline_has_its_own_line() {
        let lines = wrap("ab\n", 1000.0, mono);
        assert_eq!(line_index(&lines, 2), 0, "before the newline");
        assert_eq!(line_index(&lines, 3), 1, "after it");
        assert_eq!(lines[1].start, 3);
    }

    #[test]
    fn vertical_movement_keeps_the_column_where_it_can() {
        let mut out = laid_out("abcdef\nghijkl", 1000.0);
        out.caret = 3; // four characters in, 30 units across
        out.move_vertical(1, false, &out.lines.clone(), &mono);
        assert_eq!(out.caret, 10, "g h i j: the fourth character of the line");
        out.move_vertical(-1, false, &out.lines.clone(), &mono);
        assert_eq!(out.caret, 3);
    }

    #[test]
    fn home_and_end_stay_on_the_visual_line() {
        let mut out = laid_out("The quick brown fox", 100.0);
        out.caret = 13; // "brown", on the second line
        out.move_to_line_edge(true, false, &out.lines.clone());
        assert_eq!(out.caret, 10);
        out.move_to_line_edge(false, false, &out.lines.clone());
        assert_eq!(out.caret, 19);
    }

    #[test]
    fn a_shift_move_extends_and_a_plain_move_collapses() {
        let mut out = laid_out("abcdef", 1000.0);
        out.caret = 2;
        out.move_horizontal(false, true);
        out.move_horizontal(false, true);
        assert_eq!(out.selection(), Some((2, 4)));
        out.move_horizontal(true, false);
        assert_eq!(out.caret, 2, "a plain arrow collapses to the near edge");
        assert_eq!(out.selection(), None);
    }

    #[test]
    fn typing_replaces_the_selection() {
        let mut out = area("hello world", 5);
        out.anchor = Some(0);
        out.insert("X");
        assert_eq!(out.value, "X world");
        assert_eq!(out.caret, 1);
        assert_eq!(out.anchor, None);
    }

    #[test]
    fn backspace_and_delete_stay_on_character_boundaries() {
        let mut out = area("a😀", 5);
        out.backspace();
        assert_eq!(out.value, "a");
        assert_eq!(out.caret, 1);
        out.insert("bc");
        out.caret = 1;
        out.delete_forward();
        assert_eq!(out.value, "ac");
    }

    #[test]
    fn max_len_counts_characters_not_bytes() {
        let mut out = area("", 0);
        out.max_len = Some(3);
        out.insert("a😀bc");
        assert_eq!(out.value, "a😀b");
        assert_eq!(out.caret, "a😀b".len());
    }

    #[test]
    fn a_js_utf16_index_maps_to_a_byte_offset() {
        assert_eq!(byte_at_utf16("a😀b", 0), 0);
        assert_eq!(byte_at_utf16("a😀b", 1), 1);
        // Inside the surrogate pair rounds past it.
        assert_eq!(byte_at_utf16("a😀b", 2), 5);
        assert_eq!(byte_at_utf16("a😀b", 3), 5);
        assert_eq!(byte_at_utf16("a😀b", 99), 6);
    }

    #[test]
    fn set_puts_the_caret_at_the_end_and_keeps_the_text() {
        let keyboard = Keyboard::default();
        let mut out = TextArea::new(1);
        out.set(&keyboard, "a note\nwith two lines");
        assert_eq!(out.value(), "a note\nwith two lines");
        assert_eq!(out.caret, out.value.len());
        assert_eq!(out.anchor, None);
    }
}
