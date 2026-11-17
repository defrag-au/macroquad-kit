//! `--reel`: a reel rendered frame by frame, the game driven through a [`Player`].
//!
//! Each output frame: the scene at its time; on a new shot, the game reset and set up for it;
//! the game advanced by the shot's rate and drawn into its own target; the scene drawn into the
//! reel's target with the game's frame in it; and that read back once, for the encoder or a still.

use macroquad::prelude::*;

use super::draw::Painter;
use super::file::{Continuity, Reel};
use super::timeline::{scene, time_of};
use crate::app::{Game, Player};
use crate::harness::{self, Args, plain_target, target_camera};
use crate::video::VideoWriter;

/// What a reel run writes.
enum Output {
    /// The whole reel, encoded (`--record out.mp4`).
    Video(String),
    /// Stills at these seconds (`--shot out.png --at 2,12.5`): one, or one per time with the
    /// time in its name (`out-12.5.png`).
    Stills { path: String, at: Vec<f32> },
}

impl Output {
    fn from_args(args: &Args) -> Result<Output, String> {
        match (&args.record, &args.shot) {
            (Some(path), None) => Ok(Output::Video(path.clone())),
            (None, Some(path)) => {
                let at = harness::flag("--at")
                    .ok_or("--reel with --shot needs --at, the seconds to take")?
                    .split(',')
                    .map(|s| {
                        s.trim()
                            .parse::<f32>()
                            .map_err(|_| format!("--at: `{s}` is not a time"))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Output::Stills {
                    path: path.clone(),
                    at,
                })
            }
            _ => Err("--reel takes --record out.mp4, or --shot out.png --at SECONDS[,…]".into()),
        }
    }
}

/// The still for `t` among `count`: the path itself for one, `name-T.png` for several.
fn still_path(path: &str, t: f32, count: usize) -> String {
    if count == 1 {
        return path.to_string();
    }
    let p = std::path::Path::new(path);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("reel");
    let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("png");
    p.with_file_name(format!("{stem}-{t}.{ext}"))
        .to_string_lossy()
        .into_owned()
}

fn fail(message: impl std::fmt::Display) -> ! {
    eprintln!("{message}");
    std::process::exit(2)
}

/// Render the reel `--reel` names, as `args` asks, and exit.
pub(crate) async fn run<G: Game>(mut player: Player<G>, args: &Args) -> ! {
    let path = args.reel.as_deref().expect("run is called for --reel");
    let reel = Reel::load(path).unwrap_or_else(|e| fail(e));
    let output = Output::from_args(args).unwrap_or_else(|e| fail(e));
    let painter = Painter::load(&reel).unwrap_or_else(|e| fail(e));
    harness::report::enable();

    let size = vec2(reel.size.0 as f32, reel.size.1 as f32);
    let target = plain_target(reel.size.0, reel.size.1);
    target.texture.set_filter(FilterMode::Linear);

    // Which frames to draw, and the last one needed.
    let last = match &output {
        Output::Video(_) => reel.frames().saturating_sub(1),
        Output::Stills { at, .. } => at
            .iter()
            .map(|t| (t * reel.fps as f32).round() as u32)
            .max()
            .unwrap_or(0)
            .min(reel.frames().saturating_sub(1)),
    };
    let mut writer = match &output {
        Output::Video(out) => Some(
            VideoWriter::open(out, reel.fps, reel.size.0 as u16, reel.size.1 as u16)
                .unwrap_or_else(|e| fail(format!("{out}: {e}"))),
        ),
        Output::Stills { .. } => None,
    };

    player.settle().await;
    let mut showing: Option<usize> = None;
    for frame in 0..=last {
        let t = time_of(&reel, frame);
        let now = scene(&reel, t);
        let shot = &reel.shots[now.shot];

        // A new shot puts the game in its condition and gives it its viewport.
        if showing != Some(now.shot) {
            showing = Some(now.shot);
            if let Some(game) = &shot.game {
                if game.from == Continuity::Reset {
                    player.reset();
                }
                for (line, set) in &game.sets {
                    player
                        .setup(set)
                        .unwrap_or_else(|e| fail(format!("{path}:{line}: {e}")));
                }
                player.resize(game.size, game.px);
                player.skip(game.skip);
            }
        }
        let game_frame = shot
            .game
            .as_ref()
            .map(|g| player.frame(g.rate / reel.fps as f32).clone());

        set_camera(&target_camera(size, Vec2::ZERO, &target));
        // The report is of the reel's frame, in its pixels: the game's own text was muted.
        harness::report::begin_screen(size);
        clear_background(BLACK);
        painter.draw(&now, size, game_frame.as_ref());
        set_default_camera();

        match (&output, writer.as_mut()) {
            (Output::Video(out), Some(w)) => {
                w.push(&target.texture.get_texture_data())
                    .unwrap_or_else(|e| fail(format!("{out}: {e}")));
            }
            (Output::Stills { path, at }, _) => {
                for &want in at {
                    if (want * reel.fps as f32).round() as u32 == frame {
                        let file = still_path(path, want, at.len());
                        let note = format!("reel {} t={want}s [{}]", reel_name(args), shot.name);
                        match harness::write_capture(&target.texture, &file, &note) {
                            Ok(report) => println!("wrote {file} and {}", report.display()),
                            Err(e) => fail(format!("{file}: {e}")),
                        }
                    }
                }
            }
            _ => {}
        }
        next_frame().await;
    }
    if let (Output::Video(out), Some(w)) = (&output, writer) {
        let frames = w.finish().unwrap_or_else(|e| fail(format!("{out}: {e}")));
        println!(
            "wrote {out} · {frames} frames at {} fps, {:.1}s, {}x{}",
            reel.fps,
            frames as f32 / reel.fps as f32,
            reel.size.0,
            reel.size.1
        );
    }
    std::process::exit(0)
}

fn reel_name(args: &Args) -> &str {
    args.reel.as_deref().unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn several_stills_are_named_by_their_time() {
        assert_eq!(still_path(".tmp/r.png", 2.0, 1), ".tmp/r.png");
        assert_eq!(still_path(".tmp/r.png", 12.5, 3), ".tmp/r-12.5.png");
    }
}
