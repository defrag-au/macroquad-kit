//! `--reel`: a reel rendered frame by frame, the game driven through a [`Player`].
//!
//! Each output frame: the scene at its time; on a new shot, the game reset and set up for it;
//! the game advanced by the shot's rate and drawn into its own target; the scene drawn into the
//! reel's target with the game's frame in it, graded if the shot is; and that read back once, for
//! the encoder or a still, or shown in the preview's window.
//!
//! Three ways to run, by what is asked for:
//!
//! - `--record out.mp4`: every frame, encoded, then a summary of the reel's checks.
//! - `--shot out.png --at 2,12.5`: stills, each with the kit's text report and the reel's checks.
//! - neither: the **preview**, a window that plays the reel in real time. Space pauses, ← and →
//!   seek a second (five with Shift), `,` and `.` step a frame, Home goes back to the start, `S`
//!   shows the safe area, and `R` reads the file again: edit, save, press R.

use macroquad::prelude::*;

use super::check::{self, Outside};
use super::draw::Painter;
use super::file::{Area, Continuity, Reel, Verb};
use super::grade::Grader;
use super::timeline::{Scene, moment, scene, time_of};
use crate::app::{Game, Player};
use crate::harness::{self, Args, plain_target, target_camera};
use crate::video::{Sound, VideoWriter};

/// What a reel run writes.
enum Output {
    /// The whole reel, encoded (`--record out.mp4`).
    Video(String),
    /// Stills at these moments (`--shot out.png --at 2,12.5,sealed`): one, or one per moment with
    /// it in its name (`out-12.5.png`, `out-sealed.png`). See [`moments`].
    Stills { path: String, at: Vec<String> },
    /// Nothing written: the reel played in a window.
    Preview,
}

impl Output {
    fn from_args(args: &Args) -> Result<Output, String> {
        match (&args.record, &args.shot) {
            (Some(path), None) => Ok(Output::Video(path.clone())),
            (None, Some(path)) => {
                let at = harness::flag("--at")
                    .ok_or("--reel with --shot needs --at: seconds, or shots' names")?
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                Ok(Output::Stills {
                    path: path.clone(),
                    at,
                })
            }
            (None, None) => Ok(Output::Preview),
            (Some(_), Some(_)) => Err("--reel takes --record or --shot, not both".into()),
        }
    }
}

/// A still asked for: what `--at` called it, and the frame it is.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Moment {
    label: String,
    frame: u32,
}

/// What `--at` asks for, as frames. A number is seconds into the reel; a shot's name is that
/// shot's last frame, where everything in it has come to rest: a card's still.
fn moments(reel: &Reel, at: &[String]) -> Result<Vec<Moment>, String> {
    let frames = reel.frames();
    at.iter()
        .map(|token| {
            let frame = match token.parse::<f32>() {
                Ok(t) => (t * reel.fps as f32).round() as u32,
                Err(_) => {
                    let index = reel
                        .shots
                        .iter()
                        .position(|s| s.name == *token)
                        .ok_or_else(|| {
                            format!("--at {token}: not a time, and no shot is called that")
                        })?;
                    let end: f32 = reel.shots[..=index].iter().map(|s| s.seconds).sum();
                    ((end * reel.fps as f32).round() as u32).saturating_sub(1)
                }
            };
            if frame >= frames {
                return Err(format!(
                    "--at {token}: the reel runs 0 to {}s",
                    reel.seconds()
                ));
            }
            Ok(Moment {
                label: token.clone(),
                frame,
            })
        })
        .collect()
}

/// The still for `label` among `count`: the path itself for one, `name-LABEL.png` for several.
fn still_path(path: &str, label: &str, count: usize) -> String {
    if count == 1 {
        return path.to_string();
    }
    let p = std::path::Path::new(path);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("reel");
    let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("png");
    p.with_file_name(format!("{stem}-{label}.{ext}"))
        .to_string_lossy()
        .into_owned()
}

fn fail(message: impl std::fmt::Display) -> ! {
    eprintln!("{message}");
    std::process::exit(2)
}

/// The game's progress through the reel: which shot it was last put in, and how many of that
/// shot's `set` actions it has been handed. The game is the one stateful thing in a reel, and
/// this is the only thing that changes it.
struct Director<'p, G: Game> {
    player: Player<G>,
    path: &'p str,
    showing: Option<usize>,
    handed: usize,
}

impl<G: Game> Director<'_, G> {
    /// Put the game in shot `index`'s condition: reset unless it continues, its `set` lines, its
    /// viewport, and the moment it opens at.
    fn enter(&mut self, reel: &Reel, index: usize) {
        self.showing = Some(index);
        self.handed = 0;
        let Some(game) = &reel.shots[index].game else {
            return;
        };
        if game.from == Continuity::Reset {
            self.player.reset();
        }
        for (line, set) in &game.sets {
            if let Err(e) = self.player.setup(set) {
                fail(format!("{}:{line}: {e}", self.path));
            }
        }
        self.player.resize(game.size, game.px);
        self.player.skip(game.skip);
    }

    /// Hand the game the `set` actions of shot `index` due by `local` it has not had yet: each
    /// once, on the first frame at or after its time.
    fn hand_due(&mut self, reel: &Reel, index: usize, local: f32) {
        let due: Vec<_> = reel.shots[index]
            .actions
            .iter()
            .filter_map(|a| match &a.verb {
                Verb::Set(set) if a.at <= local => Some((a.line, set)),
                _ => None,
            })
            .collect();
        for (line, set) in due.iter().skip(self.handed) {
            if let Err(e) = self.player.setup(set) {
                fail(format!("{}:{line}: {e}", self.path));
            }
        }
        self.handed = due.len();
    }

    /// The game ready for `now`: in its shot, with what is due handed over.
    fn sync(&mut self, reel: &Reel, now: &Scene) {
        if self.showing != Some(now.shot) {
            self.enter(reel, now.shot);
        }
        self.hand_due(reel, now.shot, now.local);
    }

    /// Advance the game `dt` reel seconds (at its shot's rate) and draw it, if the shot has one.
    fn frame(&mut self, reel: &Reel, index: usize, dt: f32) -> Option<Texture2D> {
        let game = reel.shots[index].game.as_ref()?;
        Some(self.player.frame(game.rate * dt).clone())
    }

    /// Put the game where it would be at `t` had the reel played to it: from the first shot of
    /// the run of `continue` shots `t` is in, replayed undrawn a frame at a time.
    fn seek(&mut self, reel: &Reel, t: f32) {
        let (index, local) = moment(reel, t);
        let mut start = index;
        while start > 0
            && reel.shots[start]
                .game
                .as_ref()
                .is_some_and(|g| g.from == Continuity::Continue)
        {
            start -= 1;
        }
        let step = 1.0 / reel.fps as f32;
        for s in start..=index {
            self.enter(reel, s);
            let shot = &reel.shots[s];
            let Some(game) = &shot.game else {
                continue;
            };
            let until = if s == index { local } else { shot.seconds };
            let frames = (until / step).round() as u32;
            for f in 0..frames {
                self.hand_due(reel, s, f as f32 * step);
                self.player.advance(game.rate * step);
            }
        }
    }
}

/// Where a reel's frame is drawn: its target, the graded target, and what draws into them.
struct Canvas {
    size: Vec2,
    target: RenderTarget,
    graded: RenderTarget,
    grader: Grader,
    painter: Painter,
}

impl Canvas {
    fn new(reel: &Reel, painter: Painter) -> Canvas {
        let target = plain_target(reel.size.0, reel.size.1);
        target.texture.set_filter(FilterMode::Linear);
        let graded = plain_target(reel.size.0, reel.size.1);
        graded.texture.set_filter(FilterMode::Linear);
        Canvas {
            size: vec2(reel.size.0 as f32, reel.size.1 as f32),
            target,
            graded,
            grader: Grader::new().unwrap_or_else(|e| fail(e)),
            painter,
        }
    }

    /// Draw `now` with the game's frame in it, graded if its shot is: the finished frame.
    fn compose(
        &self,
        reel: &Reel,
        now: &Scene,
        game: Option<&Texture2D>,
        frame: u32,
    ) -> &RenderTarget {
        set_camera(&target_camera(self.size, Vec2::ZERO, &self.target));
        // The report is of the reel's frame, in its pixels: the game's own text was muted.
        harness::report::begin_screen(self.size);
        clear_background(BLACK);
        // Offline, as the game's frame is: every glyph is cached as it is drawn, so no caption is
        // held back a frame, and a shot with no game (which would flush the gate) still has its
        // text.
        crate::text::ungated(|| self.painter.draw(now, self.size, game));
        let finished = match &reel.shots[now.shot].grade {
            Some(grade) => {
                set_camera(&target_camera(self.size, Vec2::ZERO, &self.graded));
                self.grader
                    .draw(&self.target.texture, self.size, grade, frame);
                &self.graded
            }
            None => &self.target,
        };
        set_default_camera();
        finished
    }

    /// The text in `now` outside the reel's safe area, if it declares one.
    fn outside(&self, reel: &Reel, now: &Scene) -> Vec<check::Drawn> {
        let Some(safe) = reel.safe else {
            return Vec::new();
        };
        let drawn = self.painter.text_boxes(now, self.size);
        check::outside(safe.inside(reel.size), &drawn)
            .into_iter()
            .cloned()
            .collect()
    }
}

/// The reel's glyph check: every missing character, one line each, `path:LINE: …`.
fn glyph_problems(reel: &Reel, painter: &Painter, path: &str) -> Vec<String> {
    check::missing_glyphs(reel, |font, c| painter.has(font, c))
        .into_iter()
        .map(|(line, message)| format!("{path}:{line}: {message}"))
        .collect()
}

/// Render the reel `--reel` names, as `args` asks, and exit.
pub(crate) async fn run<G: Game>(player: Player<G>, args: &Args) -> ! {
    let path = args.reel.as_deref().expect("run is called for --reel");
    let reel = Reel::load(path).unwrap_or_else(|e| fail(e));
    let output = Output::from_args(args).unwrap_or_else(|e| fail(e));
    let wanted = match &output {
        Output::Stills { at, .. } => moments(&reel, at).unwrap_or_else(|e| fail(e)),
        _ => Vec::new(),
    };
    let painter = Painter::load(&reel).unwrap_or_else(|e| fail(e));
    let glyphs = glyph_problems(&reel, &painter, path);
    harness::report::enable();
    let mut director = Director {
        player,
        path,
        showing: None,
        handed: 0,
    };
    director.player.settle().await;
    let canvas = Canvas::new(&reel, painter);
    match output {
        Output::Preview => preview(reel, canvas, director, glyphs).await,
        _ if !glyphs.is_empty() => fail(glyphs.join("\n")),
        Output::Video(out) => record(&reel, &canvas, &mut director, &out, args).await,
        Output::Stills { path, .. } => {
            stills(&reel, &canvas, &mut director, &path, &wanted, args).await
        }
    }
}

/// Every cue in the reel, in reel time: a shot's cues start when its shot does, and a sound on an
/// action starts when the action does.
fn reel_sounds(reel: &Reel) -> Vec<Sound> {
    let mut out = Vec::new();
    let mut start = 0.0;
    for shot in &reel.shots {
        for action in &shot.actions {
            if let Some(cue) = &action.sound {
                push_sound(reel, &mut out, &cue.name, cue.volume, start + action.at);
            }
        }
        for cue in &shot.sounds {
            push_sound(reel, &mut out, &cue.name, cue.volume, start + cue.at);
        }
        start += shot.seconds;
    }
    out
}

/// One sound in reel time. The parser refuses a cue without a `sound` line, so the path is there.
fn push_sound(reel: &Reel, out: &mut Vec<Sound>, name: &str, volume: f32, at: f32) {
    if let Some(path) = reel.sounds.get(name) {
        out.push(Sound {
            path: path.clone(),
            at,
            volume,
        });
    }
}

async fn record<G: Game>(
    reel: &Reel,
    canvas: &Canvas,
    director: &mut Director<'_, G>,
    out: &str,
    args: &Args,
) -> ! {
    let sounds = reel_sounds(reel);
    let mut writer = VideoWriter::open(
        out,
        reel.fps,
        reel.size.0 as u16,
        reel.size.1 as u16,
        args.quality,
        &sounds,
    )
    .unwrap_or_else(|e| fail(format!("{out}: {e}")));
    let mut outside = Outside::default();
    let step = 1.0 / reel.fps as f32;
    for frame in 0..reel.frames() {
        let t = time_of(reel, frame);
        let now = scene(reel, t);
        director.sync(reel, &now);
        let game = director.frame(reel, now.shot, step);
        let finished = canvas.compose(reel, &now, game.as_ref(), frame);
        let strays = canvas.outside(reel, &now);
        outside.note(now.shot, t, step, &strays.iter().collect::<Vec<_>>());
        writer
            .push(&finished.texture.get_texture_data())
            .unwrap_or_else(|e| fail(format!("{out}: {e}")));
        next_frame().await;
    }
    let frames = writer
        .finish()
        .unwrap_or_else(|e| fail(format!("{out}: {e}")));
    println!(
        "wrote {out} · {frames} frames at {} fps, {:.1}s, {}x{}",
        reel.fps,
        frames as f32 / reel.fps as f32,
        reel.size.0,
        reel.size.1
    );
    let path = args.reel.as_deref().unwrap_or("");
    for line in outside.lines(reel, path) {
        eprintln!("{line}");
    }
    if !outside.is_empty() && args.fail_on_problems {
        std::process::exit(2);
    }
    std::process::exit(0)
}

async fn stills<G: Game>(
    reel: &Reel,
    canvas: &Canvas,
    director: &mut Director<'_, G>,
    path: &str,
    at: &[Moment],
    args: &Args,
) -> ! {
    let last = at.iter().map(|m| m.frame).max().unwrap_or(0);
    let step = 1.0 / reel.fps as f32;
    let mut problems = false;
    for frame in 0..=last {
        let t = time_of(reel, frame);
        let now = scene(reel, t);
        director.sync(reel, &now);
        let game = director.frame(reel, now.shot, step);
        let finished = canvas.compose(reel, &now, game.as_ref(), frame);
        for want in at.iter().filter(|m| m.frame == frame) {
            let file = still_path(path, &want.label, at.len());
            let shot = &reel.shots[now.shot].name;
            let note = format!(
                "reel {} t={t:.2}s [{shot}]",
                args.reel.as_deref().unwrap_or("")
            );
            let report = harness::write_capture(&finished.texture, &file, &note)
                .unwrap_or_else(|e| fail(format!("{file}: {e}")));
            let outside = canvas.outside(reel, &now);
            problems |= !outside.is_empty();
            append_checks(&report, reel, &outside)
                .unwrap_or_else(|e| fail(format!("{}: {e}", report.display())));
            println!("wrote {file} and {}", report.display());
        }
        next_frame().await;
    }
    if problems && args.fail_on_problems {
        std::process::exit(2);
    }
    std::process::exit(0)
}

/// Add the reel's own checks to a still's report.
fn append_checks(
    report: &std::path::Path,
    reel: &Reel,
    outside: &[check::Drawn],
) -> std::io::Result<()> {
    use std::io::Write;
    let mut text = String::from("\n## Reel checks\n");
    match reel.safe {
        None => text.push_str("No safe area declared (`safe top=… right=… bottom=… left=…`).\n"),
        Some(safe) => {
            let s = safe.inside(reel.size);
            if outside.is_empty() {
                text.push_str(&format!(
                    "All text inside the safe area ({:.0}, {:.0}, {:.0}×{:.0}).\n",
                    s.x, s.y, s.w, s.h
                ));
            }
            for d in outside {
                text.push_str(&format!(
                    "- outside the safe area: line {} {:?} at ({:.0}, {:.0}, {:.0}×{:.0})\n",
                    d.line,
                    check::short(&d.text),
                    d.area.x,
                    d.area.y,
                    d.area.w,
                    d.area.h
                ));
            }
        }
    }
    std::fs::OpenOptions::new()
        .append(true)
        .open(report)?
        .write_all(text.as_bytes())
}

/// The reel in a window, in real time, until the window closes or `Q` is pressed.
async fn preview<G: Game>(
    mut reel: Reel,
    mut canvas: Canvas,
    mut director: Director<'_, G>,
    mut notes: Vec<String>,
) -> ! {
    let path = director.path;
    for note in &notes {
        eprintln!("{note}");
    }
    let mut t = 0.0_f32;
    let mut playing = true;
    let mut show_safe = true;
    director.seek(&reel, t);
    loop {
        let dt = get_frame_time().min(0.1);
        let end = (reel.seconds() - 1.0 / reel.fps as f32).max(0.0);
        let shift = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);
        let jump = if shift { 5.0 } else { 1.0 };
        let mut seek_to = None;
        if is_key_pressed(KeyCode::Space) {
            playing = !playing;
            if playing && t >= end {
                seek_to = Some(0.0);
            }
        }
        if is_key_pressed(KeyCode::Left) {
            seek_to = Some(t - jump);
        }
        if is_key_pressed(KeyCode::Right) {
            seek_to = Some(t + jump);
        }
        if is_key_pressed(KeyCode::Comma) {
            playing = false;
            seek_to = Some(t - 1.0 / reel.fps as f32);
        }
        if is_key_pressed(KeyCode::Period) {
            playing = false;
            seek_to = Some(t + 1.0 / reel.fps as f32);
        }
        if is_key_pressed(KeyCode::Home) {
            seek_to = Some(0.0);
        }
        if is_key_pressed(KeyCode::S) {
            show_safe = !show_safe;
        }
        if is_key_pressed(KeyCode::Q) || is_key_pressed(KeyCode::Escape) {
            std::process::exit(0);
        }
        if is_key_pressed(KeyCode::R) {
            // Read the file again: a mistake keeps the reel that was showing, and says why.
            match Reel::load(path).and_then(|r| Painter::load(&r).map(|p| (r, p))) {
                Ok((fresh, painter)) => {
                    notes = glyph_problems(&fresh, &painter, path);
                    notes.insert(0, format!("read {path} again"));
                    canvas = Canvas::new(&fresh, painter);
                    reel = fresh;
                    seek_to = Some(t);
                }
                Err(e) => notes = vec![e],
            }
            for note in &notes {
                eprintln!("{note}");
            }
        }

        let end = (reel.seconds() - 1.0 / reel.fps as f32).max(0.0);
        let advance = match seek_to {
            Some(to) => {
                t = to.clamp(0.0, end);
                director.seek(&reel, t);
                0.0
            }
            None if playing => {
                let next = (t + dt).min(end);
                let moved = next - t;
                t = next;
                if t >= end {
                    playing = false;
                }
                moved
            }
            None => 0.0,
        };
        let now = scene(&reel, t);
        director.sync(&reel, &now);
        let game = director.frame(&reel, now.shot, advance);
        let frame = (t * reel.fps as f32).round() as u32;
        let finished = canvas.compose(&reel, &now, game.as_ref(), frame);
        let outside = canvas.outside(&reel, &now);

        // The frame, fitted to the window and letterboxed, and what the preview says over it.
        clear_background(Color::new(0.05, 0.05, 0.05, 1.0));
        let window = vec2(screen_width(), screen_height());
        let hud = 54.0;
        let room = vec2(window.x, (window.y - hud).max(1.0));
        let scale = (room.x / canvas.size.x).min(room.y / canvas.size.y);
        let shown = canvas.size * scale;
        let origin = vec2((window.x - shown.x) / 2.0, hud + (room.y - shown.y) / 2.0);
        draw_texture_ex(
            &finished.texture,
            origin.x,
            origin.y,
            WHITE,
            DrawTextureParams {
                dest_size: Some(shown),
                flip_y: true,
                ..Default::default()
            },
        );
        let in_window = |a: Area| {
            Rect::new(
                origin.x + a.x * scale,
                origin.y + a.y * scale,
                a.w * scale,
                a.h * scale,
            )
        };
        if show_safe && let Some(safe) = reel.safe {
            let s = in_window(safe.inside(reel.size));
            draw_rectangle_lines(s.x, s.y, s.w, s.h, 1.0, Color::new(1.0, 1.0, 1.0, 0.35));
        }
        for d in &outside {
            let r = in_window(d.area);
            draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, Color::new(1.0, 0.25, 0.25, 0.9));
        }
        let state = if playing { "playing" } else { "paused" };
        let shot = &reel.shots[now.shot].name;
        draw_text(
            format!(
                "{t:6.2}s / {:.1}s  [{shot}]  {state}  ·  space  ←/→ (shift ×5)  , .  home  S safe  R reload  Q quit",
                reel.seconds()
            ),
            10.0,
            20.0,
            18.0,
            WHITE,
        );
        let mut status = notes.clone();
        status.extend(outside.iter().map(|d| {
            format!(
                "line {}: outside the safe area: {:?}",
                d.line,
                check::short(&d.text)
            )
        }));
        if let Some(first) = status.first() {
            let more = if status.len() > 1 {
                format!("  (+{} more)", status.len() - 1)
            } else {
                String::new()
            };
            draw_text(
                format!("{first}{more}"),
                10.0,
                42.0,
                16.0,
                Color::new(1.0, 0.6, 0.4, 1.0),
            );
        }
        next_frame().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn several_stills_are_named_by_their_time() {
        assert_eq!(still_path(".tmp/r.png", "2", 1), ".tmp/r.png");
        assert_eq!(still_path(".tmp/r.png", "12.5", 3), ".tmp/r-12.5.png");
        assert_eq!(
            still_path("out/card.png", "sealed", 5),
            "out/card-sealed.png"
        );
    }

    #[test]
    fn a_shots_name_is_its_last_frame_and_a_number_is_seconds() {
        let reel = Reel::parse("reel 10x10 30\n[problem] 2\n[sealed] 3").unwrap();
        let at = |s: &[&str]| moments(&reel, &s.iter().map(|t| t.to_string()).collect::<Vec<_>>());
        assert_eq!(
            at(&["problem", "sealed", "2.5"]).unwrap(),
            [
                Moment {
                    label: "problem".into(),
                    frame: 59
                },
                Moment {
                    label: "sealed".into(),
                    frame: 149
                },
                Moment {
                    label: "2.5".into(),
                    frame: 75
                },
            ]
        );
        assert_eq!(
            at(&["finale"]).unwrap_err(),
            "--at finale: not a time, and no shot is called that"
        );
        assert_eq!(at(&["9"]).unwrap_err(), "--at 9: the reel runs 0 to 5s");
    }
}
