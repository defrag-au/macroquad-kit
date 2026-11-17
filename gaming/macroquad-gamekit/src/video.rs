//! `VideoWriter` — a plan's `record` clip, encoded as it is drawn.
//!
//! Each frame the run captures for a clip goes straight down a pipe to `ffmpeg` as raw RGBA, and
//! comes out as one H.264 MP4: no folder of PNGs to assemble afterwards. The fixed clock is what
//! makes this a recording rather than a screen capture — every frame is drawn whole, at the
//! plan's size, however long it took, so the clip plays at true speed and the same every run.
//!
//! The encoder is the `ffmpeg` on the `PATH`, or the binary `MQ_FFMPEG` names; without one the
//! clip fails to open, naming what to install (`nix shell nixpkgs#ffmpeg-headless` brings one).
//! Native only: a browser has no process to pipe to.

/// Rows bottom-first, as a render target reads back (`Image` from `get_texture_data`), turned
/// top-down, with alpha set opaque: what the encoder is told the frame is. A capture is opaque
/// for the reason a still is (`harness::save_texture_png`): a translucent pixel in a target is
/// the backdrop showing, not a hole.
pub fn upright_opaque(bytes: &[u8], width: usize, height: usize) -> Vec<u8> {
    let row = width * 4;
    let mut out = Vec::with_capacity(row * height);
    for y in (0..height).rev() {
        let start = y * row;
        out.extend_from_slice(&bytes[start..start + row]);
    }
    for alpha in out.iter_mut().skip(3).step_by(4) {
        *alpha = 255;
    }
    out
}

/// How hard a clip is compressed: what it is for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Quality {
    /// Near transparent (`-crf 16`): the copy to keep, cut or re-encode. Large: grain and tape
    /// noise are what an encoder spends the most on, so an 84 s portrait reel with them is
    /// about 140 MB.
    #[default]
    Master,
    /// For posting (`-crf 26`): the same reel in about 9 MB, under Discord's 10 MB, with UI text
    /// still crisp and the grain a little smoother. Side by side with a master frame, the
    /// difference is hard to find.
    Share,
}

impl Quality {
    /// `--quality master` or `--quality share`.
    pub fn parse(s: &str) -> Option<Quality> {
        match s {
            "master" => Some(Quality::Master),
            "share" => Some(Quality::Share),
            _ => None,
        }
    }

    /// x264's constant rate factor: lower is closer to the frames and larger.
    pub fn crf(self) -> u32 {
        match self {
            Quality::Master => 16,
            Quality::Share => 26,
        }
    }
}

/// The `ffmpeg` arguments that encode `width`×`height` RGBA frames at `fps` into `path`.
///
/// `yuv420p` is what every player and site plays, and it needs even sides, so an odd side is
/// padded by one pixel rather than refused. The rate factor is `quality`'s; `+faststart` lets a
/// page start playing before the file has fully arrived.
pub fn ffmpeg_args(path: &str, fps: u32, width: u32, height: u32, quality: Quality) -> Vec<String> {
    [
        "-y",
        "-loglevel",
        "error",
        "-f",
        "rawvideo",
        "-pix_fmt",
        "rgba",
        "-s",
        &format!("{width}x{height}"),
        "-r",
        &fps.to_string(),
        "-i",
        "-",
        "-an",
        "-vf",
        "pad=ceil(iw/2)*2:ceil(ih/2)*2",
        "-c:v",
        "libx264",
        "-preset",
        "slow",
        "-crf",
        &quality.crf().to_string(),
        "-pix_fmt",
        "yuv420p",
        "-movflags",
        "+faststart",
        path,
    ]
    .iter()
    .map(|arg| arg.to_string())
    .collect()
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use std::io::{self, Write};
    use std::path::Path;
    use std::process::{Child, ChildStdin, Command, Stdio};

    use macroquad::prelude::Image;

    /// One clip being encoded. See the [module docs](super).
    pub struct VideoWriter {
        path: String,
        width: u16,
        height: u16,
        child: Child,
        stdin: Option<ChildStdin>,
        frames: u32,
    }

    impl VideoWriter {
        /// Start encoding `width`×`height` frames at `fps` into `path` at `quality`, making its
        /// directory.
        pub fn open(
            path: &str,
            fps: u32,
            width: u16,
            height: u16,
            quality: super::Quality,
        ) -> io::Result<Self> {
            if let Some(parent) = Path::new(path)
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
            {
                std::fs::create_dir_all(parent)?;
            }
            let ffmpeg = std::env::var("MQ_FFMPEG").unwrap_or_else(|_| "ffmpeg".into());
            let mut child = Command::new(&ffmpeg)
                .args(super::ffmpeg_args(
                    path,
                    fps,
                    width.into(),
                    height.into(),
                    quality,
                ))
                .stdin(Stdio::piped())
                .spawn()
                .map_err(|e| {
                    io::Error::new(
                        e.kind(),
                        format!(
                            "cannot run `{ffmpeg}` ({e}); a clip needs ffmpeg on the PATH or \
                             MQ_FFMPEG — `nix shell nixpkgs#ffmpeg-headless` brings one"
                        ),
                    )
                })?;
            let stdin = child.stdin.take();
            Ok(Self {
                path: path.to_string(),
                width,
                height,
                child,
                stdin,
                frames: 0,
            })
        }

        /// Add a frame, as a target reads back. Every frame is the clip's size: a window that
        /// changed size mid-clip is refused rather than encoded as noise.
        pub fn push(&mut self, image: &Image) -> io::Result<()> {
            if (image.width, image.height) != (self.width, self.height) {
                return Err(io::Error::other(format!(
                    "{}: frame {} is {}x{}, the clip is {}x{}",
                    self.path, self.frames, image.width, image.height, self.width, self.height
                )));
            }
            let frame = super::upright_opaque(&image.bytes, self.width.into(), self.height.into());
            self.stdin
                .as_mut()
                .ok_or_else(|| io::Error::other("the clip is already finished"))?
                .write_all(&frame)?;
            self.frames += 1;
            Ok(())
        }

        /// Close the clip and wait for it to be written. Returns how many frames it holds.
        pub fn finish(mut self) -> io::Result<u32> {
            drop(self.stdin.take());
            let status = self.child.wait()?;
            if !status.success() {
                return Err(io::Error::other(format!(
                    "{}: ffmpeg exited with {status}",
                    self.path
                )));
            }
            Ok(self.frames)
        }

        /// Its size, for the run's report.
        pub fn size(&self) -> (u16, u16) {
            (self.width, self.height)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use imp::VideoWriter;

/// The browser has no process to pipe to: a clip there is refused when it would open.
#[cfg(target_arch = "wasm32")]
pub struct VideoWriter;

#[cfg(target_arch = "wasm32")]
use std::io;

#[cfg(target_arch = "wasm32")]
impl VideoWriter {
    pub fn open(_: &str, _: u32, _: u16, _: u16, _: Quality) -> io::Result<Self> {
        Err(io::Error::other(
            "a clip is recorded natively, not in a browser",
        ))
    }
    pub fn push(&mut self, _: &macroquad::prelude::Image) -> io::Result<()> {
        Ok(())
    }
    pub fn finish(self) -> io::Result<u32> {
        Ok(0)
    }
    pub fn size(&self) -> (u16, u16) {
        (0, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two rows of two pixels, bottom row first: the top row comes out first, all opaque.
    #[test]
    fn frames_go_to_the_encoder_upright_and_opaque() {
        let bottom = [1, 1, 1, 0, 2, 2, 2, 10];
        let top = [3, 3, 3, 128, 4, 4, 4, 255];
        let read_back: Vec<u8> = bottom.iter().chain(&top).copied().collect();
        assert_eq!(
            upright_opaque(&read_back, 2, 2),
            vec![3, 3, 3, 255, 4, 4, 4, 255, 1, 1, 1, 255, 2, 2, 2, 255]
        );
    }

    #[test]
    fn the_encoder_is_told_the_frame_and_pads_odd_sides() {
        let args = ffmpeg_args("out/reveal.mp4", 30, 1080, 1919, Quality::Master);
        let joined = args.join(" ");
        assert!(joined.contains("-f rawvideo -pix_fmt rgba -s 1080x1919 -r 30 -i -"));
        assert!(joined.contains("pad=ceil(iw/2)*2:ceil(ih/2)*2"));
        assert!(joined.contains("-c:v libx264"));
        assert!(joined.contains("-crf 16"));
        assert!(joined.contains("-pix_fmt yuv420p"));
        assert_eq!(args.last().map(String::as_str), Some("out/reveal.mp4"));
    }

    #[test]
    fn a_share_copy_trades_size_for_a_little_grain() {
        let joined = ffmpeg_args("s.mp4", 30, 1080, 1920, Quality::Share).join(" ");
        assert!(joined.contains("-crf 26"));
        assert_eq!(Quality::parse("share"), Some(Quality::Share));
        assert_eq!(Quality::parse("master"), Some(Quality::Master));
        assert_eq!(Quality::parse("best"), None);
        assert_eq!(Quality::default(), Quality::Master);
    }
}
