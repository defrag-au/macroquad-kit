//! `reel` — a game's promo video, rendered by the game from a `.reel` file: its own screens in phone frames, items placed and moved by timed actions, encoded once.
//!
//! See `REEL.md` beside the crate for the format and why it exists. In brief: a shot puts the
//! game in a condition (`set` lines, as a capture plan's), *places* items (a fill, an image, the
//! game full-frame, a phone showing it), and *actions* move them (`2.5 move @ph 0.45 y=-240`).
//!
//! - [`file`] parses and checks a reel. Pure.
//! - [`timeline`] is the reel at a time: each item's pose, the placement with every action due
//!   folded over it. Pure, so any frame is exact and nothing is stepped.
//! - [`draw`] draws a scene with macroquad, each item under its pose's model matrix.
//! - `run` is `--reel`: the game driven through a narrow player, the frames read back once.
//!
//! ```sh
//! game --reel promo/clone-registry.reel --record out/clone-registry.mp4
//! game --reel promo/clone-registry.reel --shot .tmp/r.png --at 2,12.5,18
//! ```

pub mod draw;
pub mod file;
pub mod grade;
pub(crate) mod run;
pub mod timeline;

pub use file::{Reel, ReelError};
