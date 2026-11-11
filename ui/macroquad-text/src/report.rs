//! What the text report records. Off until [`enable`]; every run drawn
//! through this crate calls in, so nothing else has to. A capture
//! (`macroquad_gamekit::harness::Capture`) writes it beside its PNG.

use std::cell::RefCell;
use std::collections::HashSet;
use std::fmt::Write as _;

use macroquad::prelude::*;

/// Below this many design units, text is reported as too small to read.
pub const MIN_TEXT: f32 = 8.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub text: String,
    /// Its visible box: left, top, width, height, in the surface's units,
    /// clipped to what is on screen.
    pub rect: Rect,
    pub size: f32,
    pub alpha: f32,
    /// This run is a fit's cut, or its last word: its trailing ellipsis is
    /// deliberate.
    pub cut: bool,
}

/// A surface text is drawn on: the screen, or a baked face.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Surface {
    pub size: Vec2,
    pub lines: Vec<Line>,
}

#[derive(Default)]
struct State {
    on: bool,
    /// Depth of [`muted`] scopes: text drawn inside one is not recorded.
    muted: u32,
    screen: Surface,
    /// Where the screen's content begins in the drawing coordinate space.
    origin: Vec2,
    /// Clips in force, in the drawing units of the surface they were opened
    /// on. Text wholly outside one is not drawn, so it is not recorded; the
    /// part inside is what a line's box comes to.
    clips: Vec<Rect>,
    /// Strings a fit produced this frame, whose trailing ellipsis is the cut
    /// its caller asked for rather than a symptom.
    fitted: HashSet<String>,
    /// Faces baked since recording began, each once, in bake order.
    bakes: Vec<Surface>,
    /// The bake being drawn, if one is.
    baking: Option<Surface>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Start recording. Call before anything is baked, so faces are caught.
pub fn enable() {
    STATE.with(|s| s.borrow_mut().on = true);
}

pub fn on() -> bool {
    STATE.with(|s| s.borrow().on)
}

/// A new screen frame of `size` units: forget the last frame's lines.
pub fn begin_screen(size: Vec2) {
    begin_screen_at(Vec2::ZERO, size);
}

/// A new screen frame of `size` units drawn at `origin`: forget the last
/// frame's lines, and report this frame's in coordinates relative to the
/// surface.
///
/// Use this when the screen is a panel inside a larger frame — a storybook
/// story, a capture crop — so the report measures against the panel rather
/// than the window it happens to be drawn in.
pub fn begin_screen_at(origin: Vec2, size: Vec2) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.origin = origin;
        s.clips.clear();
        s.fitted.clear();
        s.screen = Surface {
            size,
            lines: Vec::new(),
        };
    });
}

/// Text drawn from here until [`end_bake`] lands on a face of `size`.
pub fn begin_bake(size: Vec2) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if s.on {
            s.baking = Some(Surface {
                size,
                lines: Vec::new(),
            });
        }
    });
}

/// Forget what this bake has drawn so far: it is drawing again.
pub fn restart_bake() {
    STATE.with(|s| {
        if let Some(b) = s.borrow_mut().baking.as_mut() {
            b.lines.clear();
        }
    });
}

pub fn end_bake() {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if let Some(b) = s.baking.take()
            && !b.lines.is_empty()
            && !s.bakes.contains(&b)
        {
            s.bakes.push(b);
        }
    });
}

/// Text clipped to a rectangle in the current camera's units is text that is
/// drawn, so record it only when it falls inside the clip. Called by
/// `macroquad_gamekit::with_clip`, which sets the scissor itself.
pub fn push_clip(rect: Rect) {
    STATE.with(|s| s.borrow_mut().clips.push(rect));
}

/// Leave the innermost clip opened by [`push_clip`].
pub fn pop_clip() {
    STATE.with(|s| {
        s.borrow_mut().clips.pop();
    });
}

/// Run `draw` without recording its text.
///
/// For text drawn into an intermediate target that is composited somewhere
/// else every frame — one half of a split screen — whose coordinates are the
/// target's, not the screen's, so it would be reported in the wrong place.
/// Draw the screen's own text (a HUD) on the screen instead, where it is
/// recorded where it lands. A face baked once and kept belongs in a bake
/// ([`begin_bake`]), not here.
pub fn muted<R>(draw: impl FnOnce() -> R) -> R {
    STATE.with(|s| s.borrow_mut().muted += 1);
    let out = draw();
    STATE.with(|s| s.borrow_mut().muted -= 1);
    out
}

/// `text` is a fit's deliberate cut, not a symptom. The kit registers here
/// from the two places that cut a string to fit — `macroquad-widgets`'
/// `Painter::fit` and `macroquad-gamekit`'s `shrink_to_fit` — so a literal
/// `…` a widget writes still reports.
///
/// Cleared with the screen: a mark lasts one frame.
pub fn fitted(text: &str) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if s.on {
            s.fitted.insert(text.to_owned());
        }
    });
}

/// Whether `word` is the whole of a fit's cut, or its last word (a layout
/// that draws a block draws it one word at a time).
fn is_fitted(fitted: &HashSet<String>, word: &str) -> bool {
    fitted.contains(word)
        || fitted.iter().any(|whole| {
            whole
                .strip_suffix(word)
                .is_some_and(|head| head.ends_with(' '))
        })
}

/// A line of text was drawn.
pub fn line(text: &str, mut rect: Rect, size: f32, alpha: f32) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if !s.on || s.muted > 0 {
            return;
        }
        // A clip shows only the part of a run inside it: record that part, so
        // text scrolled under its box is measured where it is seen, and a run
        // half past the window's edge is not off its surface. A clip with no
        // area draws nothing at all.
        for clip in &s.clips {
            if clip.w <= 0.0 || clip.h <= 0.0 {
                return;
            }
            match clip.intersect(rect) {
                Some(visible) if visible.w > 0.0 && visible.h > 0.0 => rect = visible,
                _ => return,
            }
        }
        let line = Line {
            text: text.to_owned(),
            rect,
            size,
            alpha,
            cut: is_fitted(&s.fitted, text),
        };
        let origin = s.origin;
        match s.baking.as_mut() {
            Some(b) => b.lines.push(line),
            None => {
                // The screen's own coordinates start at its origin: a story
                // drawn at (238, 24) in a window reports from (0, 0).
                let mut line = line;
                line.rect.x -= origin.x;
                line.rect.y -= origin.y;
                s.screen.lines.push(line);
            }
        }
    });
}

/// The screen and the faces, as recorded so far.
pub fn surfaces() -> (Surface, Vec<Surface>) {
    STATE.with(|s| {
        let s = s.borrow();
        (s.screen.clone(), s.bakes.clone())
    })
}

/// One problem a reviewer would flag.
#[derive(Clone, Debug, PartialEq)]
pub enum Problem {
    /// Not wholly on its surface.
    Off { line: Line },
    /// Cut short with a trailing ellipsis.
    Cut { line: Line },
    /// Below [`MIN_TEXT`].
    Small { line: Line },
    /// On top of another line.
    Overlap { a: Line, b: Line },
}

/// The four classes of problem, by the name `--allow` takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProblemKind {
    Off,
    Cut,
    Small,
    Overlap,
}

impl ProblemKind {
    pub const ALL: [ProblemKind; 4] = [
        ProblemKind::Off,
        ProblemKind::Cut,
        ProblemKind::Small,
        ProblemKind::Overlap,
    ];

    /// The name `--allow` takes.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Cut => "cut",
            Self::Small => "small",
            Self::Overlap => "overlap",
        }
    }

    /// The kind `--allow` names, or `None` for something else.
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
    }
}

impl Problem {
    /// Which class this problem is, for `--allow`.
    pub fn kind(&self) -> ProblemKind {
        match self {
            Self::Off { .. } => ProblemKind::Off,
            Self::Cut { .. } => ProblemKind::Cut,
            Self::Small { .. } => ProblemKind::Small,
            Self::Overlap { .. } => ProblemKind::Overlap,
        }
    }
}

/// Text that can be seen: fully transparent lines are animation states.
fn visible(l: &Line) -> bool {
    l.alpha > 0.05
}

/// The visible text on `surface` as a reader sees it: a layout places
/// each word on its own, so words on one baseline, in one size and a
/// space apart, are joined back into the line they make.
pub fn joined(surface: &Surface) -> Vec<Line> {
    let mut words: Vec<&Line> = surface.lines.iter().filter(|l| visible(l)).collect();
    words.sort_by(|a, b| {
        (a.rect.y.round(), a.rect.x)
            .partial_cmp(&(b.rect.y.round(), b.rect.x))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut out: Vec<Line> = Vec::new();
    for w in words {
        if let Some(last) = out.last_mut() {
            let gap = w.rect.x - last.rect.right();
            let same_line = (last.rect.y - w.rect.y).abs() < 0.5
                && (last.size - w.size).abs() < 0.01
                && gap >= -0.5
                && gap < w.size * 0.6;
            if same_line {
                last.text = format!("{} {}", last.text, w.text);
                last.rect.w = w.rect.right() - last.rect.x;
                last.rect.h = last.rect.h.max(w.rect.h);
                // Either word may be the fitted one.
                last.cut |= w.cut;
                continue;
            }
        }
        out.push(w.clone());
    }
    out
}

/// What is wrong with the text on `surface`.
pub fn problems(surface: &Surface) -> Vec<Problem> {
    let mut out = Vec::new();
    let bounds = Rect::new(0.0, 0.0, surface.size.x, surface.size.y);
    let joined = joined(surface);
    let lines: Vec<&Line> = joined.iter().collect();
    for l in &lines {
        let r = l.rect;
        let inside = r.x >= bounds.x - 0.5
            && r.y >= bounds.y - 0.5
            && r.right() <= bounds.right() + 0.5
            && r.bottom() <= bounds.bottom() + 0.5;
        if !inside {
            out.push(Problem::Off { line: (*l).clone() });
        }
        if l.text.ends_with('…') && !l.cut {
            out.push(Problem::Cut { line: (*l).clone() });
        }
        if l.size < MIN_TEXT {
            out.push(Problem::Small { line: (*l).clone() });
        }
    }
    for (i, a) in lines.iter().enumerate() {
        for b in &lines[i + 1..] {
            // A shadow or outline copy is not a second line.
            if a.text == b.text && a.rect.overlaps(&b.rect) {
                continue;
            }
            if let Some(o) = a.rect.intersect(b.rect) {
                let smaller = (a.rect.w * a.rect.h).min(b.rect.w * b.rect.h).max(1e-3);
                if o.w * o.h / smaller > 0.25 {
                    out.push(Problem::Overlap {
                        a: (*a).clone(),
                        b: (*b).clone(),
                    });
                }
            }
        }
    }
    out
}

fn rect(r: Rect) -> String {
    format!("({:.0}, {:.0}, {:.0}×{:.0})", r.x, r.y, r.w, r.h)
}

fn describe(p: &Problem, surface: &str) -> String {
    match p {
        Problem::Off { line } => format!(
            "off its surface: {:?} at {} on {surface}",
            line.text,
            rect(line.rect)
        ),
        Problem::Cut { line } => format!("cut short: {:?} on {surface}", line.text),
        Problem::Small { line } => format!(
            "too small: {:?} at {:.1} (minimum {MIN_TEXT}) on {surface}",
            line.text, line.size
        ),
        Problem::Overlap { a, b } => format!(
            "overlapping: {:?} at {} and {:?} at {} on {surface}",
            a.text,
            rect(a.rect),
            b.text,
            rect(b.rect)
        ),
    }
}

fn list(out: &mut String, surface: &Surface) {
    let mut lines = joined(surface);
    lines.dedup_by(|a, b| a.text == b.text && a.rect.overlaps(&b.rect));
    for l in lines {
        let _ = writeln!(out, "- {} {:.1} {:?}", rect(l.rect), l.size, l.text);
    }
}

/// The report for the capture written to `png`.
pub fn render(png: &str, note: &str) -> String {
    let (screen, bakes) = surfaces();
    let mut found: Vec<String> = problems(&screen)
        .iter()
        .map(|p| describe(p, "the screen"))
        .collect();
    for (i, b) in bakes.iter().enumerate() {
        let name = format!("face #{}", i + 1);
        found.extend(problems(b).iter().map(|p| describe(p, &name)));
    }
    let mut out = String::new();
    let _ = writeln!(out, "# Frame report: {png}");
    if !note.is_empty() {
        let _ = writeln!(out, "{note}");
    }
    let _ = writeln!(
        out,
        "\nBoxes are (left, top, width×height) in design units, clipped to what is visible, then \
         the text size and the text. The screen is {:.0}×{:.0}. A face's text is in the face's own \
         units; the face is drawn scaled, so its on-screen size is its size times that scale.",
        screen.size.x, screen.size.y
    );
    let _ = writeln!(out, "\n## Problems ({})", found.len());
    if found.is_empty() {
        let _ = writeln!(out, "None found.");
    }
    for p in &found {
        let _ = writeln!(out, "- {p}");
    }
    let _ = writeln!(out, "\n## Screen text, top to bottom");
    list(&mut out, &screen);
    let _ = writeln!(out, "\n## Baked faces ({})", bakes.len());
    for (i, b) in bakes.iter().enumerate() {
        let _ = writeln!(
            out,
            "\n### Face #{} ({:.0}×{:.0})",
            i + 1,
            b.size.x,
            b.size.y
        );
        list(&mut out, b);
    }
    out
}

/// Forget everything recorded, and stop recording. For tests.
#[doc(hidden)]
pub fn reset() {
    STATE.with(|s| *s.borrow_mut() = State::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen_at(origin: Vec2, size: Vec2) {
        reset();
        enable();
        begin_screen_at(origin, size);
    }

    #[test]
    fn a_screen_origin_reports_lines_relative_to_the_surface() {
        screen_at(vec2(238.0, 24.0), vec2(390.0, 844.0));
        line("inside", Rect::new(258.0, 44.0, 60.0, 16.0), 12.0, 1.0);
        let (screen, _) = surfaces();
        assert_eq!(screen.lines[0].rect, Rect::new(20.0, 20.0, 60.0, 16.0));
        assert!(problems(&screen).is_empty());
        reset();
    }

    #[test]
    fn text_left_of_the_origin_is_off_its_surface() {
        screen_at(vec2(238.0, 24.0), vec2(390.0, 844.0));
        line("left of it", Rect::new(220.0, 44.0, 40.0, 16.0), 12.0, 1.0);
        let (screen, _) = surfaces();
        assert!(matches!(
            problems(&screen).as_slice(),
            [Problem::Off { .. }]
        ));
        reset();
    }

    #[test]
    fn a_bake_keeps_its_own_coordinates() {
        screen_at(vec2(238.0, 24.0), vec2(390.0, 844.0));
        begin_bake(vec2(64.0, 64.0));
        line("face", Rect::new(8.0, 8.0, 20.0, 10.0), 10.0, 1.0);
        end_bake();
        let (screen, bakes) = surfaces();
        assert!(screen.lines.is_empty());
        assert_eq!(bakes[0].lines[0].rect, Rect::new(8.0, 8.0, 20.0, 10.0));
        reset();
    }

    #[test]
    fn text_wholly_outside_a_clip_is_not_recorded() {
        screen_at(Vec2::ZERO, vec2(200.0, 200.0));
        push_clip(Rect::new(0.0, 0.0, 100.0, 100.0));
        line("inside", Rect::new(10.0, 10.0, 40.0, 14.0), 12.0, 1.0);
        line("below", Rect::new(10.0, 140.0, 40.0, 14.0), 12.0, 1.0);
        pop_clip();
        let (screen, _) = surfaces();
        assert_eq!(screen.lines.len(), 1);
        assert_eq!(screen.lines[0].text, "inside");
        reset();
    }

    #[test]
    fn text_past_a_clip_is_measured_where_it_is_visible() {
        screen_at(Vec2::ZERO, vec2(100.0, 100.0));
        push_clip(Rect::new(0.0, 0.0, 100.0, 100.0));
        // A run that would run 30 past the surface's right edge.
        line("half past", Rect::new(90.0, 10.0, 40.0, 14.0), 12.0, 1.0);
        pop_clip();
        let (screen, _) = surfaces();
        assert_eq!(screen.lines[0].rect, Rect::new(90.0, 10.0, 10.0, 14.0));
        assert!(
            problems(&screen).is_empty(),
            "the visible part is wholly on its surface"
        );
        reset();
    }

    #[test]
    fn a_fitted_ellipsis_is_not_a_cut() {
        screen_at(Vec2::ZERO, vec2(200.0, 100.0));
        fitted("A name far…");
        line("A name far…", Rect::new(0.0, 0.0, 60.0, 14.0), 12.0, 1.0);
        assert!(problems(&surfaces().0).is_empty());
        reset();
    }

    #[test]
    fn a_fit_mark_lasts_one_frame() {
        screen_at(Vec2::ZERO, vec2(200.0, 100.0));
        fitted("A name far…");
        // The next frame draws a literal ellipsis without a fit: it reports.
        begin_screen(Vec2::new(200.0, 100.0));
        line("A name far…", Rect::new(0.0, 0.0, 60.0, 14.0), 12.0, 1.0);
        assert!(matches!(
            problems(&surfaces().0).as_slice(),
            [Problem::Cut { .. }]
        ));
        reset();
    }

    #[test]
    fn every_problem_kind_round_trips_through_its_name() {
        for kind in ProblemKind::ALL {
            assert_eq!(ProblemKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(ProblemKind::parse(" clipp "), None);
        assert_eq!(ProblemKind::parse(""), None);
    }

    #[test]
    fn a_problem_names_its_kind() {
        let line = Line {
            text: "x".to_owned(),
            rect: Rect::new(0.0, 0.0, 1.0, 1.0),
            size: 12.0,
            alpha: 1.0,
            cut: false,
        };
        let other = line.clone();
        assert_eq!(Problem::Off { line }.kind(), ProblemKind::Off);
        assert_eq!(
            Problem::Cut {
                line: other.clone()
            }
            .kind(),
            ProblemKind::Cut
        );
        assert_eq!(
            Problem::Small {
                line: other.clone()
            }
            .kind(),
            ProblemKind::Small
        );
        assert_eq!(
            Problem::Overlap {
                a: other.clone(),
                b: other
            }
            .kind(),
            ProblemKind::Overlap
        );
    }

    #[test]
    fn a_middle_elision_is_not_a_cut() {
        screen_at(Vec2::ZERO, vec2(200.0, 100.0));
        line("0f3a…9c2e", Rect::new(0.0, 0.0, 60.0, 14.0), 12.0, 1.0);
        assert!(problems(&surfaces().0).is_empty());
        reset();
    }

    #[test]
    fn a_trailing_ellipsis_is_a_cut() {
        screen_at(Vec2::ZERO, vec2(200.0, 100.0));
        line("A name far…", Rect::new(0.0, 0.0, 60.0, 14.0), 12.0, 1.0);
        assert!(matches!(
            problems(&surfaces().0).as_slice(),
            [Problem::Cut { .. }]
        ));
        reset();
    }
}
