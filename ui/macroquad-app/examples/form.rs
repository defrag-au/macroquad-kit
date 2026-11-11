//! A form on `macroquad-app`: three fields, a validation error under the
//! handle, a page of settings that scrolls, and a Continue that stays disabled
//! until the handle is valid.
//!
//! ```sh
//! cargo run -p macroquad-app --example form
//! cargo run -p macroquad-app --example form -- --shot .tmp/form.png --size 390x844
//! # a focused field with the keyboard band, and an invalid handle.
//! # .tmp/form-plan.txt:
//! #   [focus]
//! #   set focus handle
//! #   set handle ada
//! #   shot 10 .tmp/form-focus.png
//! cargo run -p macroquad-app --example form -- --plan .tmp/form-plan.txt \
//!     --size 390x844 --insets 47,0,34,0,300 --fail-on-problems
//! ```
//!
//! `--shot` and `--plan` together give the shot and never run the plan's `set`
//! lines; use one or the other.
//!
//! A plan reaches a state through `Game::setup`, one `set` line at a time:
//! `focus name|handle|city` (a focused field, with its caret), and
//! `name <text>`, `handle <text>`, `city <text>`.
//!
//! On the web the field is edited through a real `<input>`. The example names
//! the face and the file the page fetches, so the input does not change weight
//! when it takes the focus; `mq-bundle --assets ui/macroquad-storybook/assets`
//! stages that file as `assets/NotoSans-Bold.ttf`.

use macroquad::prelude::*;
use macroquad_app::{AppBar, FIELD_H, FieldKind, Insets, Keyboard, Page, TextField, shell_layout};
use macroquad_gamekit::{Backdrop, Frame, Game, Stage, clip::with_clip, harness};
use macroquad_widgets::{Align, Painter, ScrollInput, TextSize, Theme};

/// The settings rows under the form.
const SETTINGS: [(&str, &str); 8] = [
    ("Email", "ada@example.com"),
    ("Time zone", "AEST · UTC+10"),
    ("Language", "English"),
    ("Currency", "AUD"),
    ("Notifications", "On"),
    ("Weekly summary", "Off"),
    ("Two-factor", "On"),
    ("Devices", "2"),
];

/// The error line's height, reserved whether or not the handle is valid, so
/// the form does not jump when it turns red.
const ERROR_H: f32 = 22.0;

struct Form {
    font: Option<Font>,
    mono: Option<Font>,
    theme: Theme,
    keyboard: Keyboard,
    name: TextField,
    handle: TextField,
    city: TextField,
    page: Page,
    insets: Insets,
}

impl Form {
    fn new(font: Option<Font>, mono: Option<Font>) -> Self {
        let keyboard = Keyboard::new();
        let mut name = TextField::new(1);
        let mut handle = TextField::new(2);
        let mut city = TextField::new(3);
        name.set(&keyboard, "Ada Lovelace");
        handle.set(&keyboard, "ada.l");
        city.set(&keyboard, "Brisbane");
        let mut form = Self {
            font,
            mono,
            theme: Theme::tokyo_night(),
            keyboard,
            name,
            handle,
            city,
            page: Page::new(),
            insets: Insets::read(),
        };
        form.keyboard
            .input_font("Noto Sans", "assets/NotoSans-Bold.ttf");
        form
    }

    /// A handle is lowercase, with a dot in the middle.
    fn handle_ok(&self) -> bool {
        let h = self.handle.value();
        !h.is_empty() && h.contains('.') && !h.starts_with('.') && !h.ends_with('.')
    }

    fn painter(&self, frame: &Frame, tap: Option<Vec2>) -> Painter<'_> {
        Painter::new(self.font.as_ref(), self.mono.as_ref(), self.theme, tap)
            .pixel_scale(frame.px)
            .at_time(frame.clock as f64)
    }

    fn screen(&mut self, frame: &Frame, body: Rect, tap: Option<Vec2>) {
        let cta_h = 52.0;
        let cta = Rect::new(body.x + 16.0, body.bottom() - cta_h, body.w - 32.0, cta_h);
        let area = Rect::new(body.x, body.y, body.w, (cta.y - 12.0 - body.y).max(0.0));

        self.keyboard.begin(frame);
        self.keyboard.clip(area);
        let input = ScrollInput::read(frame.input.gesture.drag, frame.pointer());
        let (top, page_tap) = self.page.begin(area, &input, tap, frame.dt);

        let valid = self.handle_ok();
        // The fields and the painter borrow disjoint fields of the form, so the
        // closure that draws the page can hold both.
        let form = &mut *self;
        let p = Painter::new(form.font.as_ref(), form.mono.as_ref(), form.theme, page_tap)
            .pixel_scale(frame.px)
            .at_time(frame.clock as f64);
        let (lx, w) = (area.x + 16.0, area.w - 32.0);
        let mut y = top;

        with_clip(area, || {
            let c = form.theme.color;
            let label = |text: &str, y: f32| {
                p.text_in(
                    text,
                    Rect::new(lx, y, w, 18.0),
                    p.size(TextSize::Sm),
                    c.text_muted,
                    Align::Start,
                );
            };

            label("Name", y);
            y += 22.0;
            form.name.show(
                &p,
                &mut form.keyboard,
                Rect::new(lx, y, w, FIELD_H),
                FieldKind::Text,
                "Your name",
            );
            y += FIELD_H + 16.0;

            label("Handle", y);
            y += 22.0;
            form.handle.show(
                &p,
                &mut form.keyboard,
                Rect::new(lx, y, w, FIELD_H),
                FieldKind::Address,
                "ada.l",
            );
            y += FIELD_H + 6.0;
            if !valid {
                p.text_in(
                    "A handle is lowercase, with a dot.",
                    Rect::new(lx, y, w, 16.0),
                    p.size(TextSize::Xs),
                    c.accent_red,
                    Align::Start,
                );
            }
            y += ERROR_H;

            label("Home city", y);
            y += 22.0;
            form.city.show(
                &p,
                &mut form.keyboard,
                Rect::new(lx, y, w, FIELD_H),
                FieldKind::Search,
                "Search a city",
            );
            y += FIELD_H + 24.0;

            for (name, value) in SETTINGS {
                let row = Rect::new(lx, y, w, 52.0);
                p.text_in(
                    name,
                    row,
                    p.size(TextSize::Md),
                    c.text_primary,
                    Align::Start,
                );
                p.text_in(value, row, p.size(TextSize::Sm), c.text_muted, Align::End);
                draw_line(lx, row.bottom(), lx + w, row.bottom(), 1.0, c.border);
                y += 52.0;
            }
        });
        self.page.end(top, y);
        self.keyboard.end();

        // The footer, outside the page: no tap that scrolled it can reach here.
        let p = self.painter(frame, tap);
        let _ = p.button("Continue", cta, valid);
    }
}

impl Game for Form {
    fn update(&mut self, _frame: &Frame) {
        self.insets = Insets::read();
    }

    fn draw(&mut self, frame: &Frame) {
        clear_background(self.theme.color.bg_primary);
        let tap = frame.input.gesture.tap;
        let layout = shell_layout(frame.viewport.size, self.insets, false);
        let p = self.painter(frame, tap);
        let _ = AppBar::new("Profile")
            .subtitle("Step 1 of 3")
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
            "focus" => {
                let id = match rest.trim() {
                    "name" => 1,
                    "handle" => 2,
                    "city" => 3,
                    other => {
                        return Err(format!(
                            "unknown field `{other}`: try `focus name`, `focus handle` or `focus city`"
                        ));
                    }
                };
                self.keyboard.focus(id);
                Ok(())
            }
            "name" => {
                self.name.set(&self.keyboard, rest);
                Ok(())
            }
            "handle" => {
                self.handle.set(&self.keyboard, rest);
                Ok(())
            }
            "city" => {
                self.city.set(&self.keyboard, rest);
                Ok(())
            }
            _ => Err(format!(
                "unknown set line `{line}`: try `focus handle`, `handle ada.l` or `city Perth`"
            )),
        }
    }
}

fn window_conf() -> Conf {
    let args = harness::Args::read();
    let (w, h) = args.window_size((390, 844));
    args.window_conf(Conf {
        window_title: "macroquad-app form".to_string(),
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
    macroquad_gamekit::app::run_on(Stage::Fluid, Form::new(font, mono)).await;
}
