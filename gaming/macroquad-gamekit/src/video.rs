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

/// A sound mixed into a clip: a file, when it starts, and how loud.
///
/// A reel's cues arrive here already in reel time — a shot's cues start when its shot does.
#[derive(Clone, Debug, PartialEq)]
pub struct Sound {
    pub path: String,
    /// Seconds from the clip's first frame.
    pub at: f32,
    /// 1.0 is the file as it is. Levels add up, so a cue over a bed wants less than 1.
    pub volume: f32,
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
///
/// With `sounds`, each is a second input, delayed to its moment, mixed with the rest and padded
/// so the picture decides when the clip ends: the audio is folded in the same encode, not muxed
/// afterwards.
pub fn ffmpeg_args(
    path: &str,
    fps: u32,
    width: u32,
    height: u32,
    quality: Quality,
    sounds: &[Sound],
) -> Vec<String> {
    let mut args: Vec<String> = [
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
    ]
    .iter()
    .map(|arg| arg.to_string())
    .collect();
    for sound in sounds {
        args.push("-i".into());
        args.push(sound.path.clone());
    }
    if sounds.is_empty() {
        args.extend(["-an", "-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2"].map(String::from));
    } else {
        let graph = sound_graph(sounds);
        args.extend([
            "-filter_complex".to_string(),
            graph,
            "-map".to_string(),
            "[v]".to_string(),
            "-map".to_string(),
            "[a]".to_string(),
        ]);
    }
    args.extend(
        [
            "-c:v",
            "libx264",
            "-preset",
            "slow",
            "-crf",
            &quality.crf().to_string(),
            "-pix_fmt",
            "yuv420p",
        ]
        .map(String::from),
    );
    if !sounds.is_empty() {
        args.extend(["-c:a", "aac", "-b:a", "192k", "-shortest"].map(String::from));
    }
    args.extend(["-movflags", "+faststart", path].map(String::from));
    args
}

/// The filter graph for a clip's picture and its sounds: the picture padded, each sound delayed
/// and levelled, and the sounds mixed. `apad` leaves the mix open-ended, so `-shortest` ends the
/// clip with the picture rather than with the last cue.
fn sound_graph(sounds: &[Sound]) -> String {
    let mut parts = vec!["[0:v]pad=ceil(iw/2)*2:ceil(ih/2)*2[v]".to_string()];
    let mut labels = Vec::new();
    for (i, sound) in sounds.iter().enumerate() {
        let ms = (sound.at.max(0.0) * 1000.0).round() as u64;
        let input = i + 1;
        let volume = sound.volume;
        parts.push(format!(
            "[{input}:a]aformat=sample_rates=48000:channel_layouts=stereo,volume={volume},adelay={ms}:all=1[a{i}]"
        ));
        labels.push(format!("[a{i}]"));
    }
    if labels.len() == 1 {
        let only = &labels[0];
        parts.push(format!("{only}apad[a]"));
    } else {
        let mix = labels.join("");
        let inputs = labels.len();
        parts.push(format!(
            "{mix}amix=inputs={inputs}:normalize=0:duration=longest,apad[a]"
        ));
    }
    parts.join(";")
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
        /// directory. `sounds` are mixed in the same encode.
        pub fn open(
            path: &str,
            fps: u32,
            width: u16,
            height: u16,
            quality: super::Quality,
            sounds: &[super::Sound],
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
                    sounds,
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
    pub fn open(_: &str, _: u32, _: u16, _: u16, _: Quality, _: &[Sound]) -> io::Result<Self> {
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
        let args = ffmpeg_args("out/reveal.mp4", 30, 1080, 1919, Quality::Master, &[]);
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
        let joined = ffmpeg_args("s.mp4", 30, 1080, 1920, Quality::Share, &[]).join(" ");
        assert!(joined.contains("-crf 26"));
        assert_eq!(Quality::parse("share"), Some(Quality::Share));
        assert_eq!(Quality::parse("master"), Some(Quality::Master));
        assert_eq!(Quality::parse("best"), None);
        assert_eq!(Quality::default(), Quality::Master);
    }

    #[test]
    fn a_silent_clip_is_encoded_as_it_was() {
        let joined = ffmpeg_args("out/silent.mp4", 30, 1080, 1920, Quality::Master, &[]).join(" ");
        assert!(joined.contains("-an -vf pad="));
        assert!(!joined.contains("-filter_complex"));
    }

    #[test]
    fn sounds_are_inputs_of_their_own_mixed_in_the_same_encode() {
        let sounds = [
            Sound {
                path: "bed.mp3".into(),
                at: 0.0,
                volume: 0.4,
            },
            Sound {
                path: "whoosh.wav".into(),
                at: 0.4,
                volume: 0.6,
            },
        ];
        let args = ffmpeg_args("out/promo.mp4", 30, 1080, 1350, Quality::Master, &sounds);
        let joined = args.join(" ");
        assert!(joined.contains("-i - -i bed.mp3 -i whoosh.wav"), "{joined}");
        assert!(joined.contains(
            "[1:a]aformat=sample_rates=48000:channel_layouts=stereo,volume=0.4,adelay=0:all=1[a0]"
        ));
        assert!(joined.contains(
            "[2:a]aformat=sample_rates=48000:channel_layouts=stereo,volume=0.6,adelay=400:all=1[a1]"
        ));
        assert!(joined.contains("[a0][a1]amix=inputs=2:normalize=0:duration=longest,apad[a]"));
        assert!(joined.contains("-map [v] -map [a]"));
        assert!(joined.contains("-c:a aac -b:a 192k -shortest"));
        assert!(!joined.contains("-an"));
    }
}
