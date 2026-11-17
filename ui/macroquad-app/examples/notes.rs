//! A note on `macroquad-app`: one title and **one** text area, at phone size.
//!
//! This is the acceptance `TextArea` exists for. Grok's `au.defrag.notes` built
//! a note body out of 24 single-line fields, and every structural cost in its
//! field notes came from that: 1,200 pt of layout before the title or the
//! buttons, a 220-character line shown as one row and an ellipsis, a three-line
//! paste flattened to one line, no selection across lines, and a Return that
//! had to be mapped to "focus the next field". The body here is a single
//! `TextArea`: it wraps, grows to a cap and then scrolls inside, Return is a
//! newline, and its caret is what `Page::reveal` keeps above the keyboard.
//!
//! ```sh
//! cargo run -p macroquad-app --example notes
//! cargo run -p macroquad-app --example notes -- --shot .tmp/notes.png --size 390x844
//! # a long note, focused at its end, with the keyboard band up.
//! # .tmp/notes-plan.txt:
//! #   [long]
//! #   title Shopping for the week
//! #   body Milk
//! #   line Eggs
//! #   line Bread
//! #   line Coffee, the good one
//! #   line Olives
//! #   focus body
//! #   shot 10 .tmp/notes-long.png
//! cargo run -p macroquad-app --example notes -- --plan .tmp/notes-plan.txt \
//!     --size 390x844 --insets 47,0,34,0,300 --fail-on-problems
//! ```
//!
//! `--shot` and `--plan` together give the shot and never run the plan's `set`
//! lines; use one or the other.
//!
//! A plan reaches a state through `Game::setup`, one `set` line at a time:
//! `focus title|body`, `title <text>`, `body <text>` (replaces what is there)
//! and `line <text>` (appends a newline and the text — the only way a line-
//! based plan can build a note with more than one line in it).

use macroquad::prelude::*;
use macroquad_app::{
    AppBar, FIELD_H, FieldKind, Insets, Keyboard, Page, TextArea, TextAreaSpec, TextField,
    shell_layout,
};
use macroquad_gamekit::{Backdrop, Frame, Game, Stage, clip::with_clip, harness};
use macroquad_widgets::{Align, Painter, ScrollInput, TextSize, Theme};

struct Notes {
    font: Option<Font>,
    mono: Option<Font>,
    theme: Theme,
    keyboard: Keyboard,
    title: TextField,
    body: TextArea,
    page: Page,
    insets: Insets,
    /// The last save, for the app bar's subtitle. A host would put this on the
    /// server's reply; the screen does not care which.
    saved: Option<String>,
}

impl Notes {
    fn new(font: Option<Font>, mono: Option<Font>) -> Self {
        let keyboard = Keyboard::new();
        let mut title = TextField::new(1);
        let mut body = TextArea::new(2);
        title.set(&keyboard, "Shopping for the week");
        body.set(
            &keyboard,
            "Milk\nEggs\nBread\nCoffee, the good one — the bag, not the tin.\nOlives",
        );
        let mut notes = Self {
            font,
            mono,
            theme: Theme::tokyo_night(),
            keyboard,
            title,
            body,
            page: Page::new(),
            insets: Insets::read(),
            saved: None,
        };
        notes
            .keyboard
            .input_font("Noto Sans", "assets/NotoSans-Bold.ttf");
        notes
    }

    fn painter(&self, frame: &Frame, tap: Option<Vec2>) -> Painter<'_> {
        Painter::new(self.font.as_ref(), self.mono.as_ref(), self.theme, tap)
            .pixel_scale(frame.px)
            .at_time(frame.clock as f64)
    }

    /// What the screen says under the area: the count the notes app needed, and
    /// the fact that the area grows instead of forcing a fixed number of rows.
    fn meta(&self) -> String {
        let value = self.body.value();
        let lines = value.lines().count().max(1);
        format!("{} characters · {lines} lines", value.chars().count())
    }

    fn screen(&mut self, frame: &Frame, body: Rect, tap: Option<Vec2>) {
        let cta_h = 52.0;
        let save = Rect::new(body.x + 16.0, body.bottom() - cta_h, body.w - 32.0, cta_h);
        let area = Rect::new(body.x, body.y, body.w, (save.y - 12.0 - body.y).max(0.0));

        self.keyboard.begin(frame);
        self.keyboard.clip(area);
        let input = ScrollInput::read(frame.input.gesture.drag, frame.pointer());
        let (top, page_tap) = self.page.begin(area, &input, tap, frame.dt);

        // The fields and the painter borrow disjoint fields, so the closure that
        // draws the page can hold both.
        let notes = &mut *self;
        let p = Painter::new(
            notes.font.as_ref(),
            notes.mono.as_ref(),
            notes.theme,
            page_tap,
        )
        .pixel_scale(frame.px)
        .at_time(frame.clock as f64);
        let (lx, w) = (area.x + 16.0, area.w - 32.0);
        let mut y = top + 8.0;
        let mut caret = None;
        let mut submitted = false;

        with_clip(area, || {
            let c = notes.theme.color;
            p.text_in(
                "Title",
                Rect::new(lx, y, w, 18.0),
                p.size(TextSize::Sm),
                c.text_muted,
                Align::Start,
            );
            y += 22.0;
            notes.title.show(
                &p,
                &mut notes.keyboard,
                Rect::new(lx, y, w, FIELD_H),
                FieldKind::Text,
                "Untitled",
            );
            y += FIELD_H + 18.0;

            p.text_in(
                "Note",
                Rect::new(lx, y, w, 18.0),
                p.size(TextSize::Sm),
                c.text_muted,
                Align::Start,
            );
            y += 24.0;
            // The height it is given is ignored: the area reports what it used.
            // It wraps, grows to `max_h`, then scrolls inside on its own.
            let reply = notes.body.show(
                &p,
                &mut notes.keyboard,
                Rect::new(lx, y, w, 0.0),
                TextAreaSpec::new(FieldKind::Text, "Write it down…")
                    .max_h((area.h * 0.6).max(160.0))
                    .max_len(4_000),
                &input,
                frame.dt,
            );
            y += notes.body.height() + 10.0;
            if reply.editing {
                caret = Some(notes.body.caret_rect());
            }
            submitted = reply.submitted;

            // Inline rather than `notes.meta()`: a method on `&self` would
            // capture the whole struct and collide with the painter's borrow of
            // its fonts.
            let meta = format!(
                "{} characters · {} lines",
                notes.body.value().chars().count(),
                notes.body.value().lines().count().max(1)
            );
            p.text_in(
                &meta,
                Rect::new(lx, y, w, 16.0),
                p.size(TextSize::Xs),
                c.text_muted,
                Align::Start,
            );
            y += 22.0;
        });

        self.page.end(top, y);
        // The caret was drawn at this frame's offset; the next `begin` moves the
        // page so it sits above the keyboard. The body is already shortened by
        // `shell_layout` for the keyboard, so the reserve is zero.
        if let Some(caret) = caret {
            self.page.reveal(area, caret, 0.0);
        }
        self.keyboard.end();

        // The footer, outside the page: no tap that scrolled it reaches here.
        let p = self.painter(frame, tap);
        let tapped = p.button("Save", save, true);
        // Cmd/Ctrl+Enter saves from inside the area, which is the phone's
        // done key and the desktop's habit.
        if tapped || submitted {
            self.saved = Some(format!("Saved · {}", self.meta()));
        }
    }
}

impl Game for Notes {
    fn update(&mut self, _frame: &Frame) {
        self.insets = Insets::read();
    }

    fn draw(&mut self, frame: &Frame) {
        clear_background(self.theme.color.bg_primary);
        let tap = frame.input.gesture.tap;
        let layout = shell_layout(frame.viewport.size, self.insets, false);
        let subtitle = self.saved.as_deref().unwrap_or("Draft · not saved");
        let p = self.painter(frame, tap);
        let _ = AppBar::new("Note")
            .subtitle(subtitle)
            .show(&p, layout.bar, layout.bar_row);
        self.screen(frame, layout.body, tap);
    }

    fn backdrop(&self) -> Backdrop {
        let c = self.theme.color;
        Backdrop {
            top: c.bg_primary,
            bottom: c.bg_primary,
            accent: c.accent,
        }
    }

    fn setup(&mut self, line: &str) -> Result<(), String> {
        let (verb, rest) = line.split_once(' ').unwrap_or((line, ""));
        match verb {
            "focus" => match rest.trim() {
                "title" => {
                    self.keyboard.focus(1);
                    Ok(())
                }
                "body" => {
                    self.keyboard.focus(2);
                    Ok(())
                }
                other => Err(format!(
                    "unknown field `{other}`: try `focus title` or `focus body`"
                )),
            },
            "title" => {
                self.title.set(&self.keyboard, rest);
                Ok(())
            }
            "body" => {
                self.body.set(&self.keyboard, rest);
                Ok(())
            }
            // A plan is line-based, so a newline cannot be typed into one. This
            // is how a multi-line note is built: `body Milk` then `line Eggs`.
            "line" => {
                let mut value = self.body.value().to_owned();
                if !value.is_empty() && !value.ends_with('\n') {
                    value.push('\n');
                }
                value.push_str(rest);
                self.body.set(&self.keyboard, &value);
                Ok(())
            }
            _ => Err(format!(
                "unknown set line `{line}`: try `focus body`, `body Milk` or `line Eggs`"
            )),
        }
    }
}

fn window_conf() -> Conf {
    let args = harness::Args::read();
    let (w, h) = args.window_size((390, 844));
    args.window_conf(Conf {
        window_title: "macroquad-app notes".to_string(),
        window_width: w,
        window_height: h,
        high_dpi: true,
        window_resizable: true,
        ..Default::default()
    })
}

#[macroquad::main(window_conf)]
async fn main() {
    let font = macroquad_gamekit::load_font(include_bytes!(
        "../../macroquad-storybook/assets/NotoSans-Bold.ttf"
    ))
    .ok();
    let mono = macroquad_gamekit::load_font(include_bytes!(
        "../../macroquad-storybook/assets/JetBrainsMono-Regular.ttf"
    ))
    .ok();
    macroquad_gamekit::app::run_on(Stage::Fluid, Notes::new(font, mono)).await;
}
