//! The `.reel` file: parsed whole, checked, and every mistake named by its line.
//!
//! Pure: no macroquad, and no file system beyond [`Reel::load`]. The format is described in
//! `REEL.md`; this is its grammar.
//!
//! - **Lines.** Blank lines are skipped. A `#` followed by a space, or ending the line, starts a
//!   comment, outside quotes. A `#` followed by anything else is a colour (`#f5c542`).
//! - **Tokens.** Words are split on whitespace. A `"quoted string"` is one token, with `\n` for a
//!   line break and `\"` for a quote, and may be a `key="value"`.
//! - **Items and actions.** A shot *places* items (`phone @ph at=266,256 width=548`), and its
//!   *actions* move them: a line that starts with a time (`2.5 move @ph 0.45 y=-240`). An item
//!   is named with `@`, and only a named item can be acted on.
//! - **Nesting.** An indented line belongs to the unindented line above it: a `game`'s `set`
//!   lines.

use std::collections::BTreeMap;
use std::fmt;

use crate::ease::Ease;

/// A colour, 8 bits a channel, straight alpha.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba(pub [u8; 4]);

impl Rgba {
    pub const BLACK: Rgba = Rgba([0, 0, 0, 255]);
    pub const WHITE: Rgba = Rgba([255, 255, 255, 255]);

    /// `#rgb`, `#rrggbb` or `#rrggbbaa`.
    pub fn hex(s: &str) -> Option<Rgba> {
        let h = s.strip_prefix('#')?;
        let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
        let nibble = |i: usize| {
            u8::from_str_radix(h.get(i..i + 1)?, 16)
                .ok()
                .map(|n| n * 17)
        };
        match h.len() {
            3 => Some(Rgba([nibble(0)?, nibble(1)?, nibble(2)?, 255])),
            6 => Some(Rgba([byte(0)?, byte(2)?, byte(4)?, 255])),
            8 => Some(Rgba([byte(0)?, byte(2)?, byte(4)?, byte(6)?])),
            _ => None,
        }
    }

    /// Each channel as 0 to 1.
    pub fn unit(self) -> [f32; 4] {
        self.0.map(|c| c as f32 / 255.0)
    }
}

/// A parsed reel.
#[derive(Clone, Debug, PartialEq)]
pub struct Reel {
    /// The output's size in pixels.
    pub size: (u32, u32),
    /// Frames a second.
    pub fps: u32,
    /// Named colours (`ink gold #f5c542`).
    pub inks: BTreeMap<String, Rgba>,
    /// Named images and their paths (`image lab app/static/scene/lab.jpg`).
    pub images: BTreeMap<String, String>,
    /// Named fonts and their paths (`font noto app/assets/Noto.ttf`).
    pub fonts: BTreeMap<String, String>,
    /// Named text styles (`style cap noto 46 #ffffff em=gold centre wrap=860`).
    pub styles: BTreeMap<String, Style>,
    /// Where text may go without a platform's own buttons and captions covering it
    /// (`safe top=220 right=120 bottom=380 left=60`); no check when not given.
    pub safe: Option<Insets>,
    pub shots: Vec<Shot>,
}

/// Margins in from each edge of the frame, in pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Insets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl Insets {
    /// What is left of a frame `size` inside the margins.
    pub fn inside(self, size: (u32, u32)) -> Area {
        Area {
            x: self.left,
            y: self.top,
            w: size.0 as f32 - self.left - self.right,
            h: size.1 as f32 - self.top - self.bottom,
        }
    }
}

/// Whether a text item is held to the reel's safe area.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Safety {
    /// It is: text outside the safe area is reported.
    #[default]
    Check,
    /// It is not (`safe=ignore`): text meant to leave the frame, a dropped phone's label.
    Ignore,
}

/// How a run of text is set: a font, a size and an ink, and how it sits.
#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    /// A font named by a `font` line.
    pub font: String,
    /// Pixels.
    pub size: f32,
    pub ink: Rgba,
    /// The ink of `*emphasised*` words; the style's own ink when not given.
    pub em: Option<Rgba>,
    /// A hard drop shadow, 4 px down.
    pub shadow: Option<Rgba>,
    pub align: Align,
    pub anchor: Anchor,
    /// The width lines wrap at, in pixels; no wrapping when not given.
    pub wrap: Option<f32>,
    /// Extra space between lines, in pixels.
    pub gap: f32,
    /// A rounded plate behind the text: a tag.
    pub plate: Option<Rgba>,
    /// The plate's or border's padding: across, then down.
    pub pad: [f32; 2],
    pub radius: f32,
    /// A border of this width in the style's ink: a stamp.
    pub border: f32,
}

/// Where a text item's `at` is across its lines: their left end, their middle, or their right
/// end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Centre,
    Right,
}

/// Where a text item's `at` is up and down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    /// The top of its first line.
    Top,
    /// The middle of its ink: what was drawn, not the line box, which leaves room below for
    /// descenders that capitals have not got. A tag and the label beside it, both `middle` at
    /// one y, share a centre line whatever their sizes.
    Middle,
}

/// A stretch of text in one weight: `*stars*` around words emphasise them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub em: bool,
}

/// `text` split into its plain and `*emphasised*` runs. An unclosed star emphasises to the end.
pub fn runs(text: &str) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    for (i, part) in text.split('*').enumerate() {
        if !part.is_empty() {
            out.push(Run {
                text: part.to_string(),
                em: i % 2 == 1,
            });
        }
    }
    out
}

/// One row of a sheet, top to bottom.
#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    /// A line of text in a style.
    Line { style: String, text: String },
    /// Text in a filled, rounded box: the message being signed.
    Boxed {
        style: String,
        text: String,
        fill: Rgba,
    },
    /// Buttons side by side, sharing the width: each outlined in the style's ink, the last
    /// filled with it.
    Buttons { style: String, labels: Vec<String> },
}

/// One shot: a length, the items it places, the actions that move them, and the game in it.
#[derive(Clone, Debug, PartialEq)]
pub struct Shot {
    pub line: usize,
    pub name: String,
    pub seconds: f32,
    /// How the reel changes into this shot from the one before.
    pub cut: Cut,
    pub game: Option<GameShot>,
    /// In drawing order, back first.
    pub items: Vec<Item>,
    /// In time order; actions at the same moment keep their file order.
    pub actions: Vec<Action>,
    /// A look over the whole shot, once it is drawn (`grade=vhs`): the shot's frame goes through
    /// it as one pass, its items and the game's screen together.
    pub grade: Option<Grade>,
}

/// A look laid over a finished frame (`grade vhs mono=1 contrast=0.25 tape=0.55 …`): every
/// control 0 when not given, which leaves the frame as it was.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Grade {
    /// 0 is the frame's colour, 1 its luminance alone.
    pub mono: f32,
    /// Stretch about mid-grey: 0.25 is a quarter more.
    pub contrast: f32,
    /// Stops: -0.15 is a little darker.
    pub exposure: f32,
    /// How dark the corners go.
    pub vignette: f32,
    /// How dark the gaps between scanlines are.
    pub scan: f32,
    /// A worn tape's texture: lines jitter sideways and grain crawls.
    pub tape: f32,
    /// A tracking band rolling down the picture every six seconds, tearing the lines it crosses
    /// sideways: the tape's loudest fault, so a control of its own.
    pub track: f32,
    /// Red and blue pulled apart sideways, as a VHS smears its colour.
    pub bleed: f32,
}

impl Grade {
    /// Its controls by name, as a `grade` line writes them.
    fn slots(&mut self) -> [(&'static str, &mut f32); 8] {
        [
            ("mono", &mut self.mono),
            ("contrast", &mut self.contrast),
            ("exposure", &mut self.exposure),
            ("vignette", &mut self.vignette),
            ("scan", &mut self.scan),
            ("tape", &mut self.tape),
            ("track", &mut self.track),
            ("bleed", &mut self.bleed),
        ]
    }
}

/// How a reel changes into a shot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cut {
    /// Straight in.
    Hard,
    /// Through a colour: it rises over the last moment of the shot before, and falls away over
    /// `seconds` of this one (`cut=dip:0.35`, `cut=dip:0.35:ground`, `cut=flash:0.35`).
    Through { colour: Rgba, seconds: f32 },
}

/// The game in a shot: the condition it is put in, its viewport, and its speed. Where it shows
/// is an item's business ([`Look::View`], a [`Look::Phone`] with [`Screen::Game`]).
#[derive(Clone, Debug, PartialEq)]
pub struct GameShot {
    pub line: usize,
    /// `@name`, without the `@`: what a `set` action names to change the game mid-shot.
    pub name: Option<String>,
    /// The game's viewport in points, as a capture's `--size`.
    pub size: (u32, u32),
    /// Pixels a point, as a capture's `--px`.
    pub px: f32,
    /// The game's clock against the reel's: 0.5 is half speed.
    pub rate: f32,
    /// Seconds the game runs on, undrawn, before the shot starts (`skip=8`): a shot that opens
    /// part way into a moment.
    pub skip: f32,
    pub from: Continuity,
    /// `set` lines, handed to the game as a capture plan's are, with their line numbers.
    pub sets: Vec<(usize, String)>,
}

/// Where a shot's game starts from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Continuity {
    /// `Game::reset`, then the shot's `set` lines.
    Reset,
    /// The game as the last shot left it (`game … continue`), then the shot's `set` lines.
    Continue,
}

/// A thing a shot places. It shows from the shot's start, unless its first action is an
/// `enter` or a `show`, and until an `exit` or a `hide` takes it away.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub line: usize,
    /// `@name`, without the `@`.
    pub name: Option<String>,
    /// The item it moves with (`in=@ph`): it is placed in the frame as any item is, then moved,
    /// turned and scaled along with its parent, about the parent's centre.
    pub parent: Option<String>,
    /// Degrees it is turned as placed (`rot=-8`), about its centre.
    pub rot: f32,
    /// A window in the frame it is cut to, whatever its pose (`within=110,290,860x1160`): text
    /// scrolling past a window shows only in it.
    pub within: Option<Area>,
    /// Whether its text is held to the reel's safe area (`safe=ignore`).
    pub safety: Safety,
    pub look: Look,
}

/// What an item looks like, and where it sits.
#[derive(Clone, Debug, PartialEq)]
pub enum Look {
    /// The whole frame.
    Fill(Fill),
    /// A named image: over the whole frame, or in `area` with its corners rounded by `radius`.
    Image {
        image: String,
        fit: Fit,
        area: Option<Area>,
        radius: f32,
    },
    /// A rectangle, filled, outlined or both: a full-frame banner, a border around a card.
    Rect {
        area: Area,
        fill: Option<Rgba>,
        stroke: Option<Rgba>,
        /// The outline's width.
        border: f32,
        radius: f32,
    },
    /// A desktop browser showing the game: a toolbar with its address and extension chips over
    /// the page. `at` is its top-left and `width` its outer width; the page below the toolbar
    /// is the game's shape.
    Browser {
        at: [f32; 2],
        width: f32,
        /// The address, set in `bar`'s style.
        url: Option<Label>,
        /// The extensions' chips, right of the address, each in `chip`'s style.
        chips: Vec<Label>,
        accent: Option<Rgba>,
    },
    /// The game, over the whole frame.
    View,
    /// A phone: `at` is its top-left, `width` its outer width; its height follows its screen's
    /// shape. `label` is a chip on its top edge naming whose browser this is.
    Phone {
        at: [f32; 2],
        width: f32,
        label: Option<Label>,
        /// The rim and the label's colour (`accent=queue`).
        accent: Option<Rgba>,
        screen: Screen,
    },
    /// Text in a style, at `at` (see [`Align`] and [`Anchor`]).
    Text {
        style: String,
        at: [f32; 2],
        text: String,
    },
    /// A finger's touch: a ring `size` across, centred on `at`. It shows only while a `pulse`
    /// swells and fades it.
    Tap { at: [f32; 2], size: f32, ink: Rgba },
    /// A sheet up from the bottom of its parent phone's screen, cut to it, over a scrim that
    /// darkens the screen as the sheet arrives: a wallet asking for a signature.
    Sheet {
        fill: Rgba,
        /// How dark the screen behind goes, 0 to 1.
        scrim: f32,
        rows: Vec<Row>,
    },
}

/// A rectangle in the frame: its top-left and size, in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// A chip on a phone's top edge, in a style (`label="MetaMask's browser" chip=chip`).
#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    pub text: String,
    pub style: String,
}

/// What a phone's screen shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    /// The shot's game.
    Game,
    /// Nothing: a dark glass of this shape in points (`blank=390x844`).
    Blank { shape: (u32, u32) },
}

/// A full-frame fill.
#[derive(Clone, Debug, PartialEq)]
pub enum Fill {
    Solid(Rgba),
    /// From `inner` at the centre to `outer` at the edges.
    Radial {
        inner: Rgba,
        outer: Rgba,
    },
    /// Top to bottom through the stops, evenly spaced.
    Linear(Vec<Rgba>),
}

/// How an image fills the frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    /// All of the frame, cropped to its shape.
    Cover,
    /// All of the image, letterboxed.
    Contain,
}

/// Something that happens to an item at a moment of its shot.
#[derive(Clone, Debug, PartialEq)]
pub struct Action {
    pub line: usize,
    /// Seconds into the shot.
    pub at: f32,
    /// The item's name, without the `@`.
    pub item: String,
    pub verb: Verb,
}

/// What an action does.
#[derive(Clone, Debug, PartialEq)]
pub enum Verb {
    /// The item arrives, from the preset's pose to where it was placed.
    Enter { preset: Preset, seconds: f32 },
    /// The item leaves, from where it is to the preset's pose, and is gone after.
    Exit { preset: Preset, seconds: f32 },
    /// The item shows, at once.
    Show,
    /// The item goes, at once.
    Hide,
    /// The item moves by `by` over `seconds`, eased, and with `yoyo` more legs there and back.
    Move {
        seconds: f32,
        by: Delta,
        ease: Ease,
        yoyo: u32,
    },
    /// The item shows, swells from [`PULSE_FROM`] to [`PULSE_TO`] of its size and fades out over
    /// `seconds`, and is gone after: a tap. An item that pulses shows only while it does.
    Pulse { seconds: f32 },
    /// A condition line handed to the shot's game, as its `set` lines are: what a tap on its
    /// screen would have done (`8 set @app identity registered.json`). Only the game takes one.
    Set(String),
}

/// A pulse's scale at its start.
pub const PULSE_FROM: f32 = 0.4;
/// A pulse's scale at its end.
pub const PULSE_TO: f32 = 1.5;

/// The ways an item arrives and leaves: each is a pose it comes from or goes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    /// No motion: there, or not.
    Cut,
    Fade,
    /// Fading while rising 20 px.
    Rise,
    /// Down from 2.4 times its size, overshooting: a stamp.
    Slam,
    /// Up from 0.6 times its size, overshooting: a badge.
    Pop,
    /// From or to a whole height below where it sits: a sheet.
    Slide,
    /// Off the bottom of the frame, turning: a dropped phone.
    Drop,
}

impl Preset {
    const ALL: [(&'static str, Preset); 7] = [
        ("cut", Preset::Cut),
        ("fade", Preset::Fade),
        ("rise", Preset::Rise),
        ("slam", Preset::Slam),
        ("pop", Preset::Pop),
        ("slide", Preset::Slide),
        ("drop", Preset::Drop),
    ];

    /// How long the preset takes when the line does not say.
    pub fn seconds(self) -> f32 {
        match self {
            Preset::Cut => 0.0,
            Preset::Fade | Preset::Rise => 0.3,
            Preset::Slam | Preset::Pop => 0.35,
            Preset::Slide => 0.45,
            Preset::Drop => 1.0,
        }
    }

    /// The pose it starts from on the way in, or ends at on the way out.
    pub fn away(self) -> Delta {
        let gone = Delta {
            alpha: 0.0,
            ..Delta::NONE
        };
        match self {
            Preset::Cut => Delta::NONE,
            Preset::Fade => gone,
            Preset::Rise => Delta { y: 20.0, ..gone },
            Preset::Slam => Delta { scale: 2.4, ..gone },
            Preset::Pop => Delta { scale: 0.6, ..gone },
            Preset::Slide => Delta {
                heights: 1.0,
                ..Delta::NONE
            },
            Preset::Drop => Delta {
                frames: 0.8,
                rot: 140.0,
                ..Delta::NONE
            },
        }
    }

    /// The curve the way in follows. The way out always accelerates away ([`Ease::QuadIn`]):
    /// a thing leaving gathers speed, it does not settle.
    pub fn ease(self) -> Ease {
        match self {
            Preset::Slam | Preset::Pop => Ease::BackOut,
            Preset::Slide => Ease::CubicOut,
            Preset::Drop => Ease::QuadIn,
            _ => Ease::QuadOut,
        }
    }
}

/// A change to an item's pose, relative to where it was placed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Delta {
    /// Across, in pixels.
    pub x: f32,
    /// Down, in pixels.
    pub y: f32,
    /// Down, in the item's own heights: a sheet sliding off its screen.
    pub heights: f32,
    /// Down, in the frame's heights: a phone falling out of the picture.
    pub frames: f32,
    /// A factor: 1 is its placed size.
    pub scale: f32,
    /// Degrees, clockwise.
    pub rot: f32,
    /// A factor: 1 is as placed.
    pub alpha: f32,
}

impl Delta {
    /// No change.
    pub const NONE: Delta = Delta {
        x: 0.0,
        y: 0.0,
        heights: 0.0,
        frames: 0.0,
        scale: 1.0,
        rot: 0.0,
        alpha: 1.0,
    };

    /// `self` at `t` of the way from [`Delta::NONE`]: offsets and turns in proportion, factors
    /// from 1.
    pub fn part(self, t: f32) -> Delta {
        Delta {
            x: self.x * t,
            y: self.y * t,
            heights: self.heights * t,
            frames: self.frames * t,
            scale: 1.0 + (self.scale - 1.0) * t,
            rot: self.rot * t,
            alpha: 1.0 + (self.alpha - 1.0) * t,
        }
    }

    /// Both changes at once: offsets and turns add, factors multiply.
    pub fn then(self, other: Delta) -> Delta {
        Delta {
            x: self.x + other.x,
            y: self.y + other.y,
            heights: self.heights + other.heights,
            frames: self.frames + other.frames,
            scale: self.scale * other.scale,
            rot: self.rot + other.rot,
            alpha: self.alpha * other.alpha,
        }
    }
}

/// A mistake in a reel, at its line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReelError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ReelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ReelError {}

impl Reel {
    /// Read and parse `path`. The error names the file as well as the line.
    pub fn load(path: &str) -> Result<Reel, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        Reel::parse(&text).map_err(|e| format!("{path}:{}: {}", e.line, e.message))
    }

    /// Parse a reel's text.
    pub fn parse(text: &str) -> Result<Reel, ReelError> {
        Parser::default().run(text)
    }

    /// The reel's length in seconds: its shots end to end.
    pub fn seconds(&self) -> f32 {
        self.shots.iter().map(|s| s.seconds).sum()
    }

    /// How many frames the reel is.
    pub fn frames(&self) -> u32 {
        (self.seconds() * self.fps as f32).round() as u32
    }
}

/// One token of a line, with whether it was a bare quoted string.
#[derive(Clone, Debug, PartialEq)]
struct Token {
    text: String,
    quoted: bool,
}

/// A line split into tokens, its comment dropped. `Err` for an unclosed quote.
fn tokens(line: &str) -> Result<Vec<Token>, String> {
    let mut out = Vec::new();
    let mut chars = line.chars().peekable();
    loop {
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        let Some(&first) = chars.peek() else { break };
        if first == '#' {
            let mut ahead = chars.clone();
            ahead.next();
            if ahead.peek().is_none_or(|c| c.is_whitespace()) {
                break;
            }
        }
        let mut text = String::new();
        let mut quoted = false;
        let mut bare = true;
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() {
                break;
            }
            chars.next();
            if c != '"' {
                text.push(c);
                bare = false;
                continue;
            }
            // A bare `"…"` token is text; `key="…"` is a value.
            quoted = bare;
            loop {
                match chars.next() {
                    None => return Err("a quote is not closed".into()),
                    Some('"') => break,
                    Some('\\') => match chars.next() {
                        Some('n') => text.push('\n'),
                        Some(other) => text.push(other),
                        None => return Err("a quote is not closed".into()),
                    },
                    Some(other) => text.push(other),
                }
            }
            bare = false;
        }
        out.push(Token { text, quoted });
    }
    Ok(out)
}

/// The words after a line's keyword: positional words, and `key=value` options.
struct Words {
    line: usize,
    positional: Vec<Token>,
    options: BTreeMap<String, String>,
}

impl Words {
    fn new(line: usize, tokens: &[Token]) -> Result<Words, ReelError> {
        let mut positional = Vec::new();
        let mut options = BTreeMap::new();
        for token in tokens {
            match token.text.split_once('=') {
                Some((key, value)) if !token.quoted && !key.is_empty() => {
                    if options.insert(key.to_string(), value.to_string()).is_some() {
                        return Err(err(line, format!("`{key}=` is given twice")));
                    }
                }
                _ => positional.push(token.clone()),
            }
        }
        Ok(Words {
            line,
            positional,
            options,
        })
    }

    fn take(&mut self, key: &str) -> Option<String> {
        self.options.remove(key)
    }

    /// The `@name` among the positional words, taken out of them.
    fn name(&mut self) -> Result<Option<String>, ReelError> {
        let Some(i) = self
            .positional
            .iter()
            .position(|t| !t.quoted && t.text.starts_with('@'))
        else {
            return Ok(None);
        };
        let name = self.positional.remove(i).text[1..].to_string();
        if name.is_empty() {
            return Err(err(self.line, "`@` needs a name after it"));
        }
        Ok(Some(name))
    }

    fn texts(&self) -> Vec<String> {
        self.positional.iter().map(|t| t.text.clone()).collect()
    }

    /// Fail on any option nobody took, naming what the line accepts.
    fn done(self, what: &str, accepts: &[&str]) -> Result<(), ReelError> {
        match self.options.keys().next() {
            None => Ok(()),
            Some(key) if accepts.is_empty() => {
                Err(err(self.line, format!("`{what}` takes no `{key}=`")))
            }
            Some(key) => Err(err(
                self.line,
                format!(
                    "`{what}` takes no `{key}=`; it takes {}",
                    accepts.join(", ")
                ),
            )),
        }
    }
}

fn err(line: usize, message: impl Into<String>) -> ReelError {
    ReelError {
        line,
        message: message.into(),
    }
}

fn number(line: usize, what: &str, s: &str) -> Result<f32, ReelError> {
    s.parse::<f32>()
        .ok()
        .filter(|n| n.is_finite())
        .ok_or_else(|| err(line, format!("{what}: `{s}` is not a number")))
}

fn size(line: usize, s: &str) -> Result<(u32, u32), ReelError> {
    s.split_once('x')
        .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
        .filter(|&(w, h): &(u32, u32)| w > 0 && h > 0)
        .ok_or_else(|| err(line, format!("`{s}` is not a size; write WIDTHxHEIGHT")))
}

fn point(line: usize, s: &str) -> Result<[f32; 2], ReelError> {
    let (x, y) = s
        .split_once(',')
        .ok_or_else(|| err(line, format!("`{s}` is not a point; write X,Y")))?;
    Ok([number(line, "x", x)?, number(line, "y", y)?])
}

/// `WxH` in pixels, fractions allowed.
fn extent(line: usize, s: &str) -> Result<[f32; 2], ReelError> {
    let (w, h) = s
        .split_once('x')
        .ok_or_else(|| err(line, format!("`{s}` is not a size; write WIDTHxHEIGHT")))?;
    let (w, h) = (number(line, "width", w)?, number(line, "height", h)?);
    if w <= 0.0 || h <= 0.0 {
        return Err(err(
            line,
            format!("`{s}` is not a size; write WIDTHxHEIGHT"),
        ));
    }
    Ok([w, h])
}

/// An area from `at=X,Y` and `size=WxH`.
fn area(line: usize, at: &str, size: &str) -> Result<Area, ReelError> {
    let [x, y] = point(line, at)?;
    let [w, h] = extent(line, size)?;
    Ok(Area { x, y, w, h })
}

/// An area written whole: `X,Y,WxH`.
fn window_area(line: usize, s: &str) -> Result<Area, ReelError> {
    let bad = || {
        err(
            line,
            format!("`{s}` is not an area; write X,Y,WIDTHxHEIGHT"),
        )
    };
    let mut parts = s.splitn(3, ',');
    let (Some(x), Some(y), Some(size)) = (parts.next(), parts.next(), parts.next()) else {
        return Err(bad());
    };
    area(line, &format!("{x},{y}"), size).map_err(|_| bad())
}

/// The ease called `name`: the kit's curve names, hyphenated (`sine-in-out`, `back-out`).
fn ease(line: usize, name: &str) -> Result<Ease, ReelError> {
    Ease::ALL
        .into_iter()
        .find(|e| e.name().replace(' ', "-") == name)
        .ok_or_else(|| {
            let all: Vec<String> = Ease::ALL
                .iter()
                .map(|e| e.name().replace(' ', "-"))
                .collect();
            err(
                line,
                format!("no ease called `{name}`; there is {}", all.join(", ")),
            )
        })
}

/// `PRESET[:SECONDS]`.
fn preset(line: usize, spec: &str) -> Result<(Preset, f32), ReelError> {
    let (name, seconds) = match spec.split_once(':') {
        Some((n, s)) => (n, Some(number(line, "length", s)?)),
        None => (spec, None),
    };
    let preset = Preset::ALL
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, p)| *p)
        .ok_or_else(|| {
            let all: Vec<&str> = Preset::ALL.iter().map(|(n, _)| *n).collect();
            err(
                line,
                format!("no preset called `{name}`; there is {}", all.join(", ")),
            )
        })?;
    Ok((preset, seconds.unwrap_or(preset.seconds())))
}

#[derive(Default)]
struct Parser {
    size: Option<(u32, u32)>,
    fps: u32,
    inks: BTreeMap<String, Rgba>,
    images: BTreeMap<String, String>,
    fonts: BTreeMap<String, String>,
    styles: BTreeMap<String, Style>,
    grades: BTreeMap<String, Grade>,
    safe: Option<Insets>,
    shots: Vec<Shot>,
    /// What an indented line belongs to.
    under: Under,
}

/// The unindented line an indented one belongs to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Under {
    #[default]
    Nothing,
    /// The shot's `game`: its `set` lines.
    Game,
    /// The shot's last item, a `sheet`: its rows.
    Sheet,
}

impl Parser {
    fn run(mut self, text: &str) -> Result<Reel, ReelError> {
        for (index, raw) in text.lines().enumerate() {
            let line = index + 1;
            let toks = tokens(raw).map_err(|e| err(line, e))?;
            let Some((head, rest)) = toks.split_first() else {
                continue;
            };
            if raw.starts_with(char::is_whitespace) {
                self.child(line, head, rest)?;
            } else {
                self.under = Under::Nothing;
                self.top(line, head, rest)?;
            }
        }
        let size = self
            .size
            .ok_or_else(|| err(1, "no `reel WIDTHxHEIGHT FPS` line"))?;
        if self.shots.is_empty() {
            return Err(err(1, "no shots: start one with `[name] SECONDS`"));
        }
        for shot in &mut self.shots {
            check(shot)?;
            // Stable: actions at one moment keep their order in the file.
            shot.actions.sort_by(|a, b| a.at.total_cmp(&b.at));
        }
        Ok(Reel {
            size,
            fps: self.fps,
            inks: self.inks,
            images: self.images,
            fonts: self.fonts,
            styles: self.styles,
            safe: self.safe,
            shots: self.shots,
        })
    }

    /// A style named by an earlier `style` line.
    fn style_named(&self, line: usize, name: &str) -> Result<String, ReelError> {
        if self.styles.contains_key(name) {
            Ok(name.to_string())
        } else {
            Err(err(
                line,
                format!("no style called `{name}`; name it with a `style {name} …` line first"),
            ))
        }
    }

    /// `safe top=PX right=PX bottom=PX left=PX`.
    fn safe(&mut self, line: usize, mut words: Words) -> Result<(), ReelError> {
        if self.safe.is_some() {
            return Err(err(line, "a second `safe` line"));
        }
        if !words.positional.is_empty() {
            return Err(err(line, "write `safe top=PX right=PX bottom=PX left=PX`"));
        }
        let mut insets = Insets::default();
        for (key, slot) in [
            ("top", &mut insets.top),
            ("right", &mut insets.right),
            ("bottom", &mut insets.bottom),
            ("left", &mut insets.left),
        ] {
            if let Some(v) = words.take(key) {
                *slot = number(line, key, &v)?;
            }
        }
        words.done("safe", &["top", "right", "bottom", "left"])?;
        self.safe = Some(insets);
        Ok(())
    }

    /// `grade NAME [mono=…] [contrast=…] [exposure=…] [vignette=…] [scan=…] [tape=…] [bleed=…]`.
    fn grade(&mut self, line: usize, mut words: Words) -> Result<(), ReelError> {
        let [name] = &words.texts()[..] else {
            return Err(err(
                line,
                "write `grade NAME mono= contrast= exposure= vignette= scan= tape= bleed=`",
            ));
        };
        let name = name.clone();
        let mut grade = Grade::default();
        let keys: Vec<&str> = grade.slots().iter().map(|(k, _)| *k).collect();
        for (key, slot) in grade.slots() {
            if let Some(v) = words.take(key) {
                *slot = number(line, key, &v)?;
            }
        }
        words.done("grade", &keys)?;
        if grade == Grade::default() {
            return Err(err(line, format!("grade `{name}` changes nothing")));
        }
        if self.grades.insert(name.clone(), grade).is_some() {
            return Err(err(line, format!("a second grade called `{name}`")));
        }
        Ok(())
    }

    /// `style NAME FONT SIZE INK [options]`.
    fn style(&mut self, line: usize, mut words: Words) -> Result<(), ReelError> {
        let positional = words.texts();
        let [name, font, size, ink, flags @ ..] = &positional[..] else {
            return Err(err(
                line,
                "write `style NAME FONT SIZE INK [left|centre|right] [options]`",
            ));
        };
        if !self.fonts.contains_key(font) {
            return Err(err(
                line,
                format!(
                    "no font called `{font}`; name it with `font {font} PATH` before the first shot"
                ),
            ));
        }
        let mut align = Align::Left;
        let mut anchor = Anchor::Top;
        for flag in flags {
            match flag.as_str() {
                "left" => align = Align::Left,
                "centre" | "center" => align = Align::Centre,
                "right" => align = Align::Right,
                "top" => anchor = Anchor::Top,
                "middle" => anchor = Anchor::Middle,
                other => {
                    return Err(err(
                        line,
                        format!(
                            "`{other}`: a style aligns left, centre or right, and top or middle"
                        ),
                    ));
                }
            }
        }
        let mut colour = |key: &str| -> Result<Option<Rgba>, ReelError> {
            words.take(key).map(|c| self.colour(line, &c)).transpose()
        };
        let em = colour("em")?;
        let shadow = colour("shadow")?;
        let plate = colour("plate")?;
        let mut num = |key: &str, default: f32| -> Result<f32, ReelError> {
            words
                .take(key)
                .map(|v| number(line, key, &v))
                .transpose()
                .map(|n| n.unwrap_or(default))
        };
        let wrap = num("wrap", 0.0)?;
        let gap = num("gap", 0.0)?;
        let radius = num("radius", 0.0)?;
        let border = num("border", 0.0)?;
        let pad = match words.take("pad") {
            Some(p) => point(line, &p)?,
            None => [0.0, 0.0],
        };
        words.done(
            "style",
            &[
                "em", "shadow", "plate", "wrap", "gap", "radius", "border", "pad",
            ],
        )?;
        let style = Style {
            font: font.clone(),
            size: number(line, "size", size)?,
            ink: self.colour(line, ink)?,
            em,
            shadow,
            align,
            anchor,
            wrap: (wrap > 0.0).then_some(wrap),
            gap,
            plate,
            pad,
            radius,
            border,
        };
        if self.styles.insert(name.clone(), style).is_some() {
            return Err(err(line, format!("a second style called `{name}`")));
        }
        Ok(())
    }

    /// A colour: a named ink, or `#hex`.
    fn colour(&self, line: usize, s: &str) -> Result<Rgba, ReelError> {
        if s.starts_with('#') {
            return Rgba::hex(s)
                .ok_or_else(|| err(line, format!("`{s}` is not a colour; write #rrggbb")));
        }
        self.inks.get(s).copied().ok_or_else(|| {
            err(
                line,
                format!("no ink called `{s}`; name it with `ink {s} #rrggbb` first"),
            )
        })
    }

    fn shot(&mut self, line: usize) -> Result<&mut Shot, ReelError> {
        self.shots.last_mut().ok_or_else(|| {
            err(
                line,
                "this belongs in a shot: start one with `[name] SECONDS`",
            )
        })
    }

    fn top(&mut self, line: usize, head: &Token, rest: &[Token]) -> Result<(), ReelError> {
        let keyword = head.text.as_str();
        if let Some(name) = keyword.strip_prefix('[') {
            return self.open_shot(line, name, rest);
        }
        if keyword.starts_with(|c: char| c.is_ascii_digit() || c == '.') {
            let at = number(line, "time", keyword)?;
            return self.action(line, at, rest);
        }
        let words = Words::new(line, rest)?;
        let positional = words.texts();
        match keyword {
            "reel" => {
                if self.size.is_some() {
                    return Err(err(line, "a second `reel` line"));
                }
                let [s, fps] = &positional[..] else {
                    return Err(err(line, "write `reel WIDTHxHEIGHT FPS`"));
                };
                self.size = Some(size(line, s)?);
                self.fps = fps
                    .parse()
                    .ok()
                    .filter(|f| (1..=120).contains(f))
                    .ok_or_else(|| err(line, format!("`{fps}` is not a frame rate")))?;
                words.done("reel", &[])
            }
            _ if self.size.is_none() => {
                Err(err(line, "a reel starts with `reel WIDTHxHEIGHT FPS`"))
            }
            "ink" => {
                let [name, hex] = &positional[..] else {
                    return Err(err(line, "write `ink NAME #rrggbb`"));
                };
                let c = Rgba::hex(hex)
                    .ok_or_else(|| err(line, format!("`{hex}` is not a colour; write #rrggbb")))?;
                self.inks.insert(name.clone(), c);
                words.done("ink", &[])
            }
            // Before the first shot, `image NAME PATH` names a file; in a shot, `image` places one.
            "font" | "image" if self.shots.is_empty() => {
                let [name, path] = &positional[..] else {
                    return Err(err(line, format!("write `{keyword} NAME PATH`")));
                };
                let map = if keyword == "font" {
                    &mut self.fonts
                } else {
                    &mut self.images
                };
                if map.insert(name.clone(), path.clone()).is_some() {
                    return Err(err(line, format!("a second {keyword} called `{name}`")));
                }
                words.done(keyword, &[])
            }
            "style" => self.style(line, words),
            "grade" => self.grade(line, words),
            "safe" => self.safe(line, words),
            "game" => self.game(line, words),
            "fill" | "image" | "view" | "phone" | "text" | "tap" | "sheet" | "rect" | "browser" => {
                self.item(line, keyword, words)
            }
            other => Err(err(line, format!("`{other}` is not a reel line"))),
        }
    }

    fn open_shot(&mut self, line: usize, name: &str, rest: &[Token]) -> Result<(), ReelError> {
        if self.size.is_none() {
            return Err(err(line, "a reel starts with `reel WIDTHxHEIGHT FPS`"));
        }
        let name = name
            .strip_suffix(']')
            .filter(|n| !n.is_empty())
            .ok_or_else(|| err(line, "write `[name] SECONDS`"))?;
        if self.shots.iter().any(|s| s.name == name) {
            return Err(err(line, format!("a second shot called `{name}`")));
        }
        let mut words = Words::new(line, rest)?;
        let [seconds] = &words.positional[..] else {
            return Err(err(line, "write `[name] SECONDS`"));
        };
        let seconds = number(line, "length", &seconds.text)?;
        if seconds <= 0.0 {
            return Err(err(line, "a shot lasts more than no time"));
        }
        let cut = match words.take("cut") {
            None => Cut::Hard,
            Some(spec) => self.cut(line, &spec)?,
        };
        let grade = words
            .take("grade")
            .map(|g| {
                self.grades.get(&g).copied().ok_or_else(|| {
                    err(
                        line,
                        format!("no grade called `{g}`; name it with a `grade {g} …` line first"),
                    )
                })
            })
            .transpose()?;
        words.done("shot", &["cut", "grade"])?;
        self.shots.push(Shot {
            line,
            name: name.to_string(),
            seconds,
            cut,
            game: None,
            items: Vec::new(),
            actions: Vec::new(),
            grade,
        });
        Ok(())
    }

    fn cut(&self, line: usize, spec: &str) -> Result<Cut, ReelError> {
        let mut parts = spec.split(':');
        let kind = parts.next().unwrap_or_default();
        let seconds = match parts.next() {
            Some(s) => number(line, "cut", s)?,
            None => 0.35,
        };
        let colour = parts.next().map(|c| self.colour(line, c)).transpose()?;
        match kind {
            "hard" => Ok(Cut::Hard),
            "dip" => Ok(Cut::Through {
                colour: colour.unwrap_or(Rgba::BLACK),
                seconds,
            }),
            "flash" => Ok(Cut::Through {
                colour: colour.unwrap_or(Rgba::WHITE),
                seconds,
            }),
            other => Err(err(
                line,
                format!("`cut={other}`: a cut is hard, dip or flash"),
            )),
        }
    }

    fn game(&mut self, line: usize, mut words: Words) -> Result<(), ReelError> {
        let name = words.name()?;
        let mut from = Continuity::Reset;
        let mut viewport = None;
        for word in &words.positional {
            match word.text.as_str() {
                "continue" => from = Continuity::Continue,
                s => viewport = Some(size(line, s)?),
            }
        }
        let size =
            viewport.ok_or_else(|| err(line, "write `game WIDTHxHEIGHT [px=N] [rate=R]`"))?;
        let px = words
            .take("px")
            .map(|v| number(line, "px", &v))
            .transpose()?
            .unwrap_or(1.0);
        let rate = words
            .take("rate")
            .map(|v| number(line, "rate", &v))
            .transpose()?
            .unwrap_or(1.0);
        if !(0.25..=8.0).contains(&px) {
            return Err(err(line, "px is between 0.25 and 8"));
        }
        if !(0.05..=8.0).contains(&rate) {
            return Err(err(line, "rate is between 0.05 and 8"));
        }
        let skip = words
            .take("skip")
            .map(|v| number(line, "skip", &v))
            .transpose()?
            .unwrap_or(0.0);
        if !(0.0..=600.0).contains(&skip) {
            return Err(err(line, "skip is between 0 and 600 seconds"));
        }
        words.done("game", &["px", "rate", "skip"])?;
        let shot = self.shot(line)?;
        if shot.game.is_some() {
            return Err(err(line, "a shot has one game"));
        }
        shot.game = Some(GameShot {
            line,
            name,
            size,
            px,
            rate,
            skip,
            from,
            sets: Vec::new(),
        });
        self.under = Under::Game;
        Ok(())
    }

    /// `text [@name] STYLE at=X,Y "…"`.
    fn text(&self, line: usize, mut words: Words) -> Result<Look, ReelError> {
        let usage = "write `text [@name] STYLE at=X,Y \"…\"`";
        let (Some(style), Some(body)) = (
            words.positional.iter().find(|t| !t.quoted),
            words.positional.iter().find(|t| t.quoted),
        ) else {
            return Err(err(line, usage));
        };
        if words.positional.len() != 2 {
            return Err(err(line, usage));
        }
        let style = self.style_named(line, &style.text)?;
        let text = body.text.clone();
        let at = point(
            line,
            &words
                .take("at")
                .ok_or_else(|| err(line, "text needs `at=X,Y`"))?,
        )?;
        words.done("text", &["at", "in", "rot"])?;
        Ok(Look::Text { style, at, text })
    }

    /// `tap [@name] at=X,Y [size=120] [ink=#fff]`.
    fn tap(&self, line: usize, mut words: Words) -> Result<Look, ReelError> {
        if !words.positional.is_empty() {
            return Err(err(line, "write `tap @name at=X,Y [size=PX] [ink=INK]`"));
        }
        let at = point(
            line,
            &words
                .take("at")
                .ok_or_else(|| err(line, "a tap needs `at=X,Y`"))?,
        )?;
        let size = words
            .take("size")
            .map(|s| number(line, "size", &s))
            .transpose()?
            .unwrap_or(120.0);
        let ink = words
            .take("ink")
            .map(|c| self.colour(line, &c))
            .transpose()?
            .unwrap_or(Rgba::WHITE);
        words.done("tap", &["at", "size", "ink", "in", "rot"])?;
        Ok(Look::Tap { at, size, ink })
    }

    /// `sheet [@name] in=@phone [fill=INK] [scrim=0.6]`, its rows indented under it.
    fn sheet(&self, line: usize, mut words: Words) -> Result<Look, ReelError> {
        if !words.positional.is_empty() {
            return Err(err(
                line,
                "write `sheet @name in=@phone [fill=INK] [scrim=0.6]`",
            ));
        }
        let fill = words
            .take("fill")
            .map(|c| self.colour(line, &c))
            .transpose()?
            .unwrap_or(Rgba([0xf3, 0xf5, 0xf2, 255]));
        let scrim = words
            .take("scrim")
            .map(|s| number(line, "scrim", &s))
            .transpose()?
            .unwrap_or(0.6)
            .clamp(0.0, 1.0);
        words.done("sheet", &["fill", "scrim", "in", "rot"])?;
        Ok(Look::Sheet {
            fill,
            scrim,
            rows: Vec::new(),
        })
    }

    /// A sheet's row: `line STYLE "…"`, `box STYLE "…" [fill=INK]` or `buttons STYLE "…" "…"`.
    fn row(&self, line: usize, head: &Token, rest: &[Token]) -> Result<Row, ReelError> {
        let mut words = Words::new(line, rest)?;
        let style = words
            .positional
            .first()
            .filter(|t| !t.quoted)
            .map(|t| t.text.clone())
            .ok_or_else(|| err(line, format!("`{}` needs a style first", head.text)))?;
        let style = self.style_named(line, &style)?;
        let texts: Vec<String> = words.positional[1..]
            .iter()
            .filter(|t| t.quoted)
            .map(|t| t.text.clone())
            .collect();
        if texts.len() + 1 != words.positional.len() {
            return Err(err(line, "a row is a style and then quoted text"));
        }
        let one = |texts: &[String]| match texts {
            [t] => Ok(t.clone()),
            _ => Err(err(line, format!("`{}` takes one quoted text", head.text))),
        };
        let row = match head.text.as_str() {
            "line" => Row::Line {
                style,
                text: one(&texts)?,
            },
            "box" => Row::Boxed {
                style,
                text: one(&texts)?,
                fill: words
                    .take("fill")
                    .map(|c| self.colour(line, &c))
                    .transpose()?
                    .unwrap_or(Rgba([0xe2, 0xe7, 0xe0, 255])),
            },
            "buttons" if !texts.is_empty() => Row::Buttons {
                style,
                labels: texts,
            },
            "buttons" => return Err(err(line, "`buttons` needs at least one label")),
            other => {
                return Err(err(
                    line,
                    format!("`{other}` is not a sheet row: line, box or buttons"),
                ));
            }
        };
        words.done(&head.text, &["fill"])?;
        Ok(row)
    }

    fn item(&mut self, line: usize, keyword: &str, mut words: Words) -> Result<(), ReelError> {
        let name = words.name()?;
        let parent = words
            .take("in")
            .map(|p| {
                p.strip_prefix('@')
                    .filter(|n| !n.is_empty())
                    .map(str::to_string)
                    .ok_or_else(|| err(line, format!("`in={p}`: name an item, as `in=@phone`")))
            })
            .transpose()?;
        let rot = words
            .take("rot")
            .map(|r| number(line, "rot", &r))
            .transpose()?
            .unwrap_or(0.0);
        let safety = match words.take("safe").as_deref() {
            None | Some("check") => Safety::Check,
            Some("ignore") => Safety::Ignore,
            Some(other) => {
                return Err(err(line, format!("`safe={other}`: check or ignore")));
            }
        };
        let within = words
            .take("within")
            .map(|w| window_area(line, &w))
            .transpose()?;
        let positional = words.texts();
        let look = match keyword {
            "fill" => {
                words.done("fill", &["in", "rot"])?;
                Look::Fill(self.fill(line, &positional)?)
            }
            "view" => {
                if !positional.is_empty() {
                    return Err(err(
                        line,
                        "write `view [@name]`: the game, over the whole frame",
                    ));
                }
                words.done("view", &["in", "rot"])?;
                Look::View
            }
            "image" => {
                let [image] = &positional[..] else {
                    return Err(err(line, "write `image [@name] IMAGE [fit=cover|contain]`"));
                };
                if !self.images.contains_key(image) {
                    return Err(err(
                        line,
                        format!(
                            "no image called `{image}`; name it with `image {image} PATH` before the first shot"
                        ),
                    ));
                }
                let fit = match words.take("fit").as_deref() {
                    None | Some("cover") => Fit::Cover,
                    Some("contain") => Fit::Contain,
                    Some(other) => {
                        return Err(err(line, format!("`fit={other}`: cover or contain")));
                    }
                };
                let area = match (words.take("at"), words.take("size")) {
                    (Some(at), Some(size)) => Some(area(line, &at, &size)?),
                    (None, None) => None,
                    _ => {
                        return Err(err(
                            line,
                            "an image in a box takes both `at=X,Y` and `size=WxH`",
                        ));
                    }
                };
                let radius = words
                    .take("radius")
                    .map(|r| number(line, "radius", &r))
                    .transpose()?
                    .unwrap_or(0.0);
                words.done(
                    "image",
                    &["fit", "at", "size", "radius", "in", "rot", "within"],
                )?;
                Look::Image {
                    image: image.clone(),
                    fit,
                    area,
                    radius,
                }
            }
            "rect" => self.rect(line, words)?,
            "browser" => self.browser(line, words)?,
            "text" => self.text(line, words)?,
            "tap" => self.tap(line, words)?,
            "sheet" => {
                self.under = Under::Sheet;
                self.sheet(line, words)?
            }
            _ => self.phone(line, words)?,
        };
        self.shot(line)?.items.push(Item {
            line,
            name,
            parent,
            rot,
            within,
            safety,
            look,
        });
        Ok(())
    }

    /// `rect [@name] at=X,Y size=WxH [fill=INK] [stroke=INK] [border=PX] [radius=PX]`.
    fn rect(&self, line: usize, mut words: Words) -> Result<Look, ReelError> {
        let usage =
            "write `rect [@name] at=X,Y size=WxH [fill=INK] [stroke=INK border=PX] [radius=PX]`";
        if !words.positional.is_empty() {
            return Err(err(line, usage));
        }
        let (Some(at), Some(size)) = (words.take("at"), words.take("size")) else {
            return Err(err(line, usage));
        };
        let area = area(line, &at, &size)?;
        let mut colour = |key: &str| -> Result<Option<Rgba>, ReelError> {
            words.take(key).map(|c| self.colour(line, &c)).transpose()
        };
        let fill = colour("fill")?;
        let stroke = colour("stroke")?;
        let mut num = |key: &str, default: f32| -> Result<f32, ReelError> {
            words
                .take(key)
                .map(|v| number(line, key, &v))
                .transpose()
                .map(|n| n.unwrap_or(default))
        };
        let border = num("border", 3.0)?;
        let radius = num("radius", 0.0)?;
        if fill.is_none() && stroke.is_none() {
            return Err(err(
                line,
                "a rect draws nothing without `fill=` or `stroke=`",
            ));
        }
        words.done(
            "rect",
            &[
                "at", "size", "fill", "stroke", "border", "radius", "in", "rot", "within",
            ],
        )?;
        Ok(Look::Rect {
            area,
            fill,
            stroke,
            border,
            radius,
        })
    }

    /// `browser [@name] at=X,Y width=W [url="…" bar=STYLE] [chips="EVM ADA" chip=STYLE]
    /// [accent=INK]`.
    fn browser(&self, line: usize, mut words: Words) -> Result<Look, ReelError> {
        if !words.positional.is_empty() {
            return Err(err(
                line,
                "write `browser [@name] at=X,Y width=W [url=\"…\" bar=STYLE]`",
            ));
        }
        let at = point(
            line,
            &words
                .take("at")
                .ok_or_else(|| err(line, "a browser needs `at=X,Y`"))?,
        )?;
        let width = number(
            line,
            "width",
            &words
                .take("width")
                .ok_or_else(|| err(line, "a browser needs `width=W`"))?,
        )?;
        let url = match (words.take("url"), words.take("bar")) {
            (Some(text), Some(style)) => Some(Label {
                text,
                style: self.style_named(line, &style)?,
            }),
            (Some(_), None) => return Err(err(line, "a browser's `url=` needs `bar=STYLE`")),
            (None, Some(_)) => return Err(err(line, "`bar=` styles a `url=`; give one")),
            (None, None) => None,
        };
        let chips = match (words.take("chips"), words.take("chip")) {
            (Some(list), Some(style)) => {
                let style = self.style_named(line, &style)?;
                list.split_whitespace()
                    .map(|text| Label {
                        text: text.to_string(),
                        style: style.clone(),
                    })
                    .collect()
            }
            (Some(_), None) => return Err(err(line, "a browser's `chips=` needs `chip=STYLE`")),
            (None, Some(_)) => return Err(err(line, "`chip=` styles `chips=`; give them")),
            (None, None) => Vec::new(),
        };
        let accent = words
            .take("accent")
            .map(|c| self.colour(line, &c))
            .transpose()?;
        words.done(
            "browser",
            &[
                "at", "width", "url", "bar", "chips", "chip", "accent", "in", "rot", "within",
            ],
        )?;
        Ok(Look::Browser {
            at,
            width,
            url,
            chips,
            accent,
        })
    }

    fn phone(&self, line: usize, mut words: Words) -> Result<Look, ReelError> {
        let at = point(
            line,
            &words
                .take("at")
                .ok_or_else(|| err(line, "a phone needs `at=X,Y`"))?,
        )?;
        let width = number(
            line,
            "width",
            &words
                .take("width")
                .ok_or_else(|| err(line, "a phone needs `width=W`"))?,
        )?;
        let label = match (words.take("label"), words.take("chip")) {
            (Some(text), Some(style)) => Some(Label {
                text,
                style: self.style_named(line, &style)?,
            }),
            (Some(_), None) => {
                return Err(err(
                    line,
                    "a phone's `label=` needs `chip=STYLE` to set it in",
                ));
            }
            (None, Some(_)) => return Err(err(line, "`chip=` styles a `label=`; give one")),
            (None, None) => None,
        };
        let accent = words
            .take("accent")
            .map(|c| self.colour(line, &c))
            .transpose()?;
        let screen = match words.take("blank") {
            Some(shape) => Screen::Blank {
                shape: size(line, &shape)?,
            },
            None => Screen::Game,
        };
        if !words.positional.is_empty() {
            return Err(err(
                line,
                "write `phone [@name] at=X,Y width=W [label=\"…\"]`",
            ));
        }
        words.done(
            "phone",
            &[
                "at", "width", "label", "chip", "accent", "blank", "in", "rot",
            ],
        )?;
        Ok(Look::Phone {
            at,
            width,
            label,
            accent,
            screen,
        })
    }

    fn fill(&self, line: usize, words: &[String]) -> Result<Fill, ReelError> {
        match words {
            [c] => Ok(Fill::Solid(self.colour(line, c)?)),
            [kind, inner, outer] if kind == "radial" => Ok(Fill::Radial {
                inner: self.colour(line, inner)?,
                outer: self.colour(line, outer)?,
            }),
            [kind, stops @ ..] if kind == "linear" && stops.len() >= 2 => Ok(Fill::Linear(
                stops
                    .iter()
                    .map(|c| self.colour(line, c))
                    .collect::<Result<_, _>>()?,
            )),
            _ => Err(err(
                line,
                "write `fill COLOUR`, `fill radial INNER OUTER` or `fill linear TOP … BOTTOM`",
            )),
        }
    }

    /// `TIME VERB @item …`.
    fn action(&mut self, line: usize, at: f32, rest: &[Token]) -> Result<(), ReelError> {
        let (verb, rest) = rest.split_first().ok_or_else(|| {
            err(
                line,
                "a time needs an action: enter, exit, show, hide or move",
            )
        })?;
        // A game's `set` takes its line as written, `=` and all.
        if verb.text == "set" {
            let (target, line_text) = match rest.split_first() {
                Some((t, more))
                    if t.text.starts_with('@') && t.text.len() > 1 && !more.is_empty() =>
                {
                    (t.text[1..].to_string(), more)
                }
                _ => return Err(err(line, "write `TIME set @game LINE`")),
            };
            let text = line_text
                .iter()
                .map(|t| t.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            self.shot(line)?.actions.push(Action {
                line,
                at,
                item: target,
                verb: Verb::Set(text),
            });
            return Ok(());
        }
        let mut words = Words::new(line, rest)?;
        let item = words.name()?.ok_or_else(|| {
            err(
                line,
                format!("`{}` needs an item to act on, as `@phone`", verb.text),
            )
        })?;
        let positional = words.texts();
        let verb = match verb.text.as_str() {
            kind @ ("enter" | "exit") => {
                let (preset, seconds) = match &positional[..] {
                    [] => (Preset::Fade, Preset::Fade.seconds()),
                    [spec] => preset(line, spec)?,
                    _ => {
                        return Err(err(
                            line,
                            format!("write `TIME {kind} @item [PRESET[:SECONDS]]`"),
                        ));
                    }
                };
                words.done(kind, &[])?;
                if kind == "enter" {
                    Verb::Enter { preset, seconds }
                } else {
                    Verb::Exit { preset, seconds }
                }
            }
            kind @ ("show" | "hide") => {
                if !positional.is_empty() {
                    return Err(err(line, format!("write `TIME {kind} @item`")));
                }
                words.done(kind, &[])?;
                if kind == "show" {
                    Verb::Show
                } else {
                    Verb::Hide
                }
            }
            "move" => {
                let [seconds] = &positional[..] else {
                    return Err(err(
                        line,
                        "write `TIME move @item SECONDS x= y= scale= rot= alpha=`",
                    ));
                };
                let seconds = number(line, "length", seconds)?;
                if seconds <= 0.0 {
                    return Err(err(line, "a move takes some time"));
                }
                let mut by = Delta::NONE;
                for (key, slot) in [
                    ("x", &mut by.x),
                    ("y", &mut by.y),
                    ("scale", &mut by.scale),
                    ("rot", &mut by.rot),
                    ("alpha", &mut by.alpha),
                ] {
                    if let Some(v) = words.take(key) {
                        *slot = number(line, key, &v)?;
                    }
                }
                if by == Delta::NONE {
                    return Err(err(
                        line,
                        "a move with nothing to move: give x=, y=, scale=, rot= or alpha=",
                    ));
                }
                let ease = words
                    .take("ease")
                    .map(|e| ease(line, &e))
                    .transpose()?
                    .unwrap_or(Ease::QuadInOut);
                let yoyo = words
                    .take("yoyo")
                    .map(|n| {
                        n.parse::<u32>()
                            .map_err(|_| err(line, format!("yoyo: `{n}` is not a count")))
                    })
                    .transpose()?
                    .unwrap_or(0);
                words.done("move", &["x", "y", "scale", "rot", "alpha", "ease", "yoyo"])?;
                Verb::Move {
                    seconds,
                    by,
                    ease,
                    yoyo,
                }
            }
            "pulse" => {
                let seconds = match &positional[..] {
                    [] => 0.55,
                    [s] => number(line, "length", s)?,
                    _ => return Err(err(line, "write `TIME pulse @item [SECONDS]`")),
                };
                words.done("pulse", &[])?;
                Verb::Pulse { seconds }
            }
            other => {
                return Err(err(
                    line,
                    format!("`{other}` is not an action: enter, exit, show, hide, move or pulse"),
                ));
            }
        };
        self.shot(line)?.actions.push(Action {
            line,
            at,
            item,
            verb,
        });
        Ok(())
    }

    fn child(&mut self, line: usize, head: &Token, rest: &[Token]) -> Result<(), ReelError> {
        match self.under {
            Under::Nothing => {
                return Err(err(
                    line,
                    "an indented line belongs under a `game` or a `sheet`; nothing above it takes one",
                ));
            }
            Under::Sheet => {
                let row = self.row(line, head, rest)?;
                let shot = self.shot(line)?;
                if let Some(Item {
                    look: Look::Sheet { rows, .. },
                    ..
                }) = shot.items.last_mut()
                {
                    rows.push(row);
                }
                return Ok(());
            }
            Under::Game => {}
        }
        if head.text != "set" {
            return Err(err(
                line,
                format!(
                    "`{}` does not belong under `game`: it takes `set` lines",
                    head.text
                ),
            ));
        }
        let text = rest
            .iter()
            .map(|t| t.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        if text.is_empty() {
            return Err(err(line, "`set` with nothing to set"));
        }
        let shot = self.shot(line)?;
        shot.game
            .as_mut()
            .expect("Under::Game follows a game line")
            .sets
            .push((line, text));
        Ok(())
    }
}

/// A shot's items and actions agree: names are unique, what is acted on exists, parents exist,
/// actions fall inside the shot, and a game is shown only where there is one.
fn check(shot: &Shot) -> Result<(), ReelError> {
    let mut names = BTreeMap::new();
    for item in &shot.items {
        if let Some(name) = &item.name
            && names.insert(name.as_str(), item.line).is_some()
        {
            return Err(err(
                item.line,
                format!("a second item called `@{name}` in `{}`", shot.name),
            ));
        }
    }
    for item in &shot.items {
        if let Some(parent) = &item.parent {
            if !names.contains_key(parent.as_str()) {
                return Err(err(
                    item.line,
                    format!("`in=@{parent}`: no item called that in `{}`", shot.name),
                ));
            }
            if item.name.as_deref() == Some(parent.as_str()) {
                return Err(err(item.line, "an item cannot be inside itself"));
            }
        }
        if let Look::Sheet { rows, .. } = &item.look {
            let in_phone = item.parent.as_ref().is_some_and(|p| {
                shot.items
                    .iter()
                    .any(|i| i.name.as_ref() == Some(p) && matches!(i.look, Look::Phone { .. }))
            });
            if !in_phone {
                return Err(err(
                    item.line,
                    "a sheet rises in a phone: give it `in=@phone`",
                ));
            }
            if rows.is_empty() {
                return Err(err(
                    item.line,
                    "a sheet with no rows: indent `line`, `box` or `buttons` lines under it",
                ));
            }
        }
        let shows_game = matches!(
            item.look,
            Look::View
                | Look::Browser { .. }
                | Look::Phone {
                    screen: Screen::Game,
                    ..
                }
        );
        if shows_game && shot.game.is_none() {
            return Err(err(
                item.line,
                format!(
                    "shot `{}` has no `game` line to show; give a phone `blank=WxH` for an empty screen",
                    shot.name
                ),
            ));
        }
    }
    let game = shot.game.as_ref().and_then(|g| g.name.as_deref());
    for action in &shot.actions {
        let on_game = game == Some(action.item.as_str());
        match (&action.verb, on_game) {
            (Verb::Set(_), true) => {}
            (Verb::Set(_), false) => {
                return Err(err(
                    action.line,
                    format!(
                        "`set` changes the shot's game: name it with `game @{} …`",
                        action.item
                    ),
                ));
            }
            (_, true) => {
                return Err(err(
                    action.line,
                    format!(
                        "`@{}` is the game, which takes `set`; move the item that shows it",
                        action.item
                    ),
                ));
            }
            (_, false) if !names.contains_key(action.item.as_str()) => {
                return Err(err(
                    action.line,
                    format!("no item called `@{}` in `{}`", action.item, shot.name),
                ));
            }
            _ => {}
        }
        if action.at >= shot.seconds {
            return Err(err(
                action.line,
                format!(
                    "at {}s, after shot `{}` ends at {}s",
                    action.at, shot.name, shot.seconds
                ),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const REEL: &str = r##"
reel 1080x1920 30
ink ground #070c09
ink gold #f5c542
image lab app/static/scene/lab-2048.jpg
font mono app/assets/Mono.ttf
style chip mono 28 ground plate=#dfffd0 pad=20,8 radius=99

[title] 3.6 cut=flash:0.35
image lab
fill @shade linear #00000040 ground
1 enter @shade fade:0.5

[step4] 14 cut=dip:0.35:ground   # into the app
fill radial #12301a ground
game 390x844 px=3 rate=1.25
  set bundle app/fixtures/bundle.json
  set identity app/fixtures/identity-climber.json
phone @ph at=266,256 width=548 label="Cardano wallet's browser" chip=chip
0.3 move @ph 0.45 y=-240 rot=-22 ease=sine-in-out yoyo=5
0   enter @ph slide
12.5 exit @ph drop
"##;

    #[test]
    fn a_reel_parses_into_shots_of_items_and_actions() {
        let reel = Reel::parse(REEL).unwrap();
        assert_eq!(reel.size, (1080, 1920));
        assert_eq!(reel.fps, 30);
        assert_eq!(reel.seconds(), 17.6);
        assert_eq!(reel.frames(), 528);

        let title = &reel.shots[0];
        assert_eq!(
            title.cut,
            Cut::Through {
                colour: Rgba::WHITE,
                seconds: 0.35
            }
        );
        assert!(title.game.is_none());
        assert_eq!(title.items[1].name.as_deref(), Some("shade"));
        assert_eq!(
            title.items[1].look,
            Look::Fill(Fill::Linear(vec![
                Rgba([0, 0, 0, 0x40]),
                Rgba([7, 12, 9, 255])
            ]))
        );
        assert_eq!(
            title.actions[0].verb,
            Verb::Enter {
                preset: Preset::Fade,
                seconds: 0.5
            }
        );

        let step = &reel.shots[1];
        let game = step.game.as_ref().unwrap();
        assert_eq!((game.size, game.px, game.rate), ((390, 844), 3.0, 1.25));
        assert_eq!(game.from, Continuity::Reset);
        assert_eq!(
            game.sets[1].1,
            "identity app/fixtures/identity-climber.json"
        );
        assert_eq!(
            step.items[1].look,
            Look::Phone {
                at: [266.0, 256.0],
                width: 548.0,
                label: Some(Label {
                    text: "Cardano wallet's browser".into(),
                    style: "chip".into(),
                }),
                accent: None,
                screen: Screen::Game,
            }
        );
        // Sorted by time, whatever order the file wrote them in.
        let times: Vec<f32> = step.actions.iter().map(|a| a.at).collect();
        assert_eq!(times, [0.0, 0.3, 12.5]);
        assert_eq!(
            step.actions[1].verb,
            Verb::Move {
                seconds: 0.45,
                by: Delta {
                    y: -240.0,
                    rot: -22.0,
                    ..Delta::NONE
                },
                ease: Ease::SineInOut,
                yoyo: 5,
            }
        );
    }

    #[test]
    fn a_comment_needs_a_space_after_its_hash_and_a_colour_does_not() {
        let t = tokens(r##"fill #070c09  # the ground"##).unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!(t[1].text, "#070c09");
        let t = tokens(r#"text "a # in quotes" #"#).unwrap();
        assert_eq!(t[1].text, "a # in quotes");
        assert!(t[1].quoted);
    }

    #[test]
    fn quotes_take_escapes_and_may_be_an_options_value() {
        let t = tokens(r#"phone label="Say \"hi\"\nthen go" "x""#).unwrap();
        assert_eq!(t[1].text, "label=Say \"hi\"\nthen go");
        assert!(!t[1].quoted);
        assert!(t[2].quoted);
        assert_eq!(tokens(r#"a "open"#).unwrap_err(), "a quote is not closed");
    }

    fn error(text: &str) -> String {
        Reel::parse(text).unwrap_err().to_string()
    }

    const HEAD: &str = "reel 10x10 30\n[a] 2\n";

    #[test]
    fn every_mistake_names_its_line() {
        let cases = [
            (
                "[a] 2",
                "line 1: a reel starts with `reel WIDTHxHEIGHT FPS`",
            ),
            (
                "reel 10x10 30",
                "line 1: no shots: start one with `[name] SECONDS`",
            ),
            (
                "fill gold",
                "line 3: no ink called `gold`; name it with `ink gold #rrggbb` first",
            ),
            (
                "  set x",
                "line 3: an indented line belongs under a `game` or a `sheet`; nothing above it takes one",
            ),
            (
                "game 390x844 speed=2",
                "line 3: `game` takes no `speed=`; it takes px, rate, skip",
            ),
            (
                "view",
                "line 3: shot `a` has no `game` line to show; give a phone `blank=WxH` for an empty screen",
            ),
            (
                "fill @x #fff\nfill @x #000",
                "line 4: a second item called `@x` in `a`",
            ),
            ("1 enter @nobody", "line 3: no item called `@nobody` in `a`"),
            (
                "fill @x #fff\n3 hide @x",
                "line 4: at 3s, after shot `a` ends at 2s",
            ),
            (
                "fill @x #fff\n1 enter @x whoosh",
                "line 4: no preset called `whoosh`; there is cut, fade, rise, slam, pop, slide, drop",
            ),
            (
                "fill @x #fff\n1 move @x 0.5",
                "line 4: a move with nothing to move: give x=, y=, scale=, rot= or alpha=",
            ),
            (
                "fill @x #fff\n1 move @x 0.5 y=3 ease=bouncy",
                "line 4: no ease called `bouncy`; there is linear, quad-in, quad-out, quad-in-out, cubic-in, cubic-out, cubic-in-out, expo-out, sine-in-out, back-out",
            ),
            (
                "fill @x #fff\n1 spin @x",
                "line 4: `spin` is not an action: enter, exit, show, hide, move or pulse",
            ),
            (
                "text cap at=1,1 \"hi\"",
                "line 3: no style called `cap`; name it with a `style cap …` line first",
            ),
            (
                "phone @ph at=0,0 width=10 blank=9x16\nsheet @s",
                "line 4: a sheet rises in a phone: give it `in=@phone`",
            ),
            (
                "phone @ph at=0,0 width=10 blank=9x16\nsheet @s in=@ph",
                "line 4: a sheet with no rows: indent `line`, `box` or `buttons` lines under it",
            ),
            (
                "fill #fff in=@y",
                "line 3: `in=@y`: no item called that in `a`",
            ),
            ("[a] 1", "line 3: a second shot called `a`"),
        ];
        for (body, want) in cases {
            let text = if body.starts_with('[') && body != "[a] 1" || body.starts_with("reel") {
                body.to_string()
            } else {
                format!("{HEAD}{body}")
            };
            assert_eq!(error(&text), want, "for {body:?}");
        }
        assert_eq!(
            error("reel 10x10 30\n[a] 2 cut=wipe"),
            "line 2: `cut=wipe`: a cut is hard, dip or flash"
        );
    }

    #[test]
    fn text_styles_taps_and_a_signing_sheet_parse() {
        let reel = Reel::parse(
            r##"
reel 1080x1920 30
ink gold #f5c542
font noto app/assets/Noto.ttf
style cap noto 46 #ffffff em=gold shadow=#000 centre wrap=860
style stamp noto 60 #8cff5a centre border=8 radius=18 pad=34,20
style btn noto 32 #101410 centre
[s] 12
phone @ph at=266,256 width=548 blank=390x844
text @c cap at=540,1470 "Open it in *MetaMask's browser*.\nThen sign."
text @ok stamp at=540,760 rot=-8 "REGISTERED ✓"
tap @t at=540,856
sheet @sign in=@ph scrim=0.5
  line cap "METAMASK"
  box cap "Register this wallet" fill=#e2e7e0
  buttons btn "Cancel" "Sign"
2.5 pulse @t
"##,
        )
        .unwrap();
        let cap = &reel.styles["cap"];
        assert_eq!(
            (cap.size, cap.align, cap.wrap),
            (46.0, Align::Centre, Some(860.0))
        );
        assert_eq!(cap.em, Some(Rgba([0xf5, 0xc5, 0x42, 255])));
        assert_eq!(reel.styles["stamp"].pad, [34.0, 20.0]);
        let items = &reel.shots[0].items;
        assert_eq!(
            items[1].look,
            Look::Text {
                style: "cap".into(),
                at: [540.0, 1470.0],
                text: "Open it in *MetaMask's browser*.\nThen sign.".into(),
            }
        );
        assert_eq!(items[2].rot, -8.0);
        let Look::Sheet { scrim, rows, .. } = &items[4].look else {
            panic!("{:?}", items[4].look)
        };
        assert_eq!(*scrim, 0.5);
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows[2],
            Row::Buttons {
                style: "btn".into(),
                labels: vec!["Cancel".into(), "Sign".into()],
            }
        );
        assert_eq!(reel.shots[0].actions[0].verb, Verb::Pulse { seconds: 0.55 });
    }

    #[test]
    fn rects_browsers_boxed_images_and_windows_parse() {
        let reel = Reel::parse(
            r##"
reel 1080x1920 30
ink gold #f5c542
font mono app/Mono.ttf
style url mono 28 #dfffd0
style ext mono 26 #070c09 plate=#dfffd0
image card app/card.jpg
[s] 8
game 1280x800 px=1.5
rect @burst at=0,0 size=1080x1920 fill=gold
browser @web at=40,520 width=1000 url="robinhood.perp.quest" bar=url chips="EVM ADA" chip=ext
image @card card at=60,470 size=960x504 radius=24
text @roll url at=110,1490 within=110,290,860x1160 "fine print"
"##,
        )
        .unwrap();
        let items = &reel.shots[0].items;
        assert_eq!(
            items[0].look,
            Look::Rect {
                area: Area {
                    x: 0.0,
                    y: 0.0,
                    w: 1080.0,
                    h: 1920.0
                },
                fill: Some(Rgba([0xf5, 0xc5, 0x42, 255])),
                stroke: None,
                border: 3.0,
                radius: 0.0,
            }
        );
        let Look::Browser { url, chips, .. } = &items[1].look else {
            panic!("{:?}", items[1].look)
        };
        assert_eq!(url.as_ref().unwrap().text, "robinhood.perp.quest");
        let chips: Vec<&str> = chips.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(chips, ["EVM", "ADA"]);
        assert!(matches!(
            items[2].look,
            Look::Image {
                area: Some(Area { w: 960.0, .. }),
                radius: 24.0,
                ..
            }
        ));
        assert_eq!(
            items[3].within,
            Some(Area {
                x: 110.0,
                y: 290.0,
                w: 860.0,
                h: 1160.0
            })
        );

        let cases = [
            (
                "rect at=0,0 size=10x10",
                "line 3: a rect draws nothing without `fill=` or `stroke=`",
            ),
            (
                "browser at=0,0 width=10 url=x",
                "line 3: a browser's `url=` needs `bar=STYLE`",
            ),
            (
                "rect at=0,0 size=10x10 fill=#fff within=1,2",
                "line 3: `1,2` is not an area; write X,Y,WIDTHxHEIGHT",
            ),
            (
                "browser at=0,0 width=10",
                "line 3: shot `a` has no `game` line to show; give a phone `blank=WxH` for an empty screen",
            ),
        ];
        for (body, want) in cases {
            assert_eq!(error(&format!("{HEAD}{body}")), want, "for {body:?}");
        }
    }

    #[test]
    fn a_safe_area_is_margins_in_from_the_edges() {
        let reel = Reel::parse(
            "reel 1080x1920 30\nsafe top=220 right=120 bottom=380 left=60\n[a] 1\nfill @x #fff safe=ignore",
        )
        .unwrap();
        let inside = reel.safe.unwrap().inside(reel.size);
        assert_eq!(
            (inside.x, inside.y, inside.w, inside.h),
            (60.0, 220.0, 900.0, 1320.0)
        );
        assert_eq!(reel.shots[0].items[0].safety, Safety::Ignore);
        assert_eq!(
            error("reel 10x10 30\n[a] 1\nfill #fff safe=maybe"),
            "line 3: `safe=maybe`: check or ignore"
        );
    }

    #[test]
    fn a_grade_is_named_then_laid_over_a_shot() {
        let reel = Reel::parse(
            "reel 10x10 30\ngrade vhs mono=1 contrast=0.25 tape=0.55\n[a] 1 grade=vhs\n[b] 1",
        )
        .unwrap();
        let g = reel.shots[0].grade.unwrap();
        assert_eq!((g.mono, g.contrast, g.tape, g.scan), (1.0, 0.25, 0.55, 0.0));
        assert_eq!(reel.shots[1].grade, None);
        assert_eq!(
            error("reel 10x10 30\n[a] 1 grade=film"),
            "line 2: no grade called `film`; name it with a `grade film …` line first"
        );
        assert_eq!(
            error("reel 10x10 30\ngrade flat\n[a] 1"),
            "line 2: grade `flat` changes nothing"
        );
        assert_eq!(
            error("reel 10x10 30\ngrade g sepia=1\n[a] 1"),
            "line 2: `grade` takes no `sepia=`; it takes mono, contrast, exposure, vignette, scan, tape, track, bleed"
        );
    }

    #[test]
    fn stars_mark_emphasis() {
        let r = runs("Open it in *MetaMask's browser*. Done");
        assert_eq!(r.len(), 3);
        assert!(!r[0].em && r[1].em && !r[2].em);
        assert_eq!(r[1].text, "MetaMask's browser");
        assert_eq!(
            runs("*all*"),
            vec![Run {
                text: "all".into(),
                em: true
            }]
        );
    }

    #[test]
    fn a_continued_game_keeps_its_state() {
        let reel =
            Reel::parse("reel 10x10 30\n[a] 1\ngame 10x10\n[b] 1\ngame 10x10 continue").unwrap();
        assert_eq!(
            reel.shots[1].game.as_ref().unwrap().from,
            Continuity::Continue
        );
    }

    #[test]
    fn colours_take_three_six_or_eight_digits() {
        assert_eq!(Rgba::hex("#fff"), Some(Rgba::WHITE));
        assert_eq!(Rgba::hex("#f5c542"), Some(Rgba([0xf5, 0xc5, 0x42, 255])));
        assert_eq!(Rgba::hex("#00000080"), Some(Rgba([0, 0, 0, 0x80])));
        assert_eq!(Rgba::hex("#ggg"), None);
        assert_eq!(Rgba::hex("fff"), None);
    }

    #[test]
    fn deltas_compose_and_scale_from_none() {
        let d = Delta {
            y: 20.0,
            scale: 2.0,
            alpha: 0.0,
            ..Delta::NONE
        };
        let half = d.part(0.5);
        assert_eq!((half.y, half.scale, half.alpha), (10.0, 1.5, 0.5));
        assert_eq!(d.part(0.0), Delta::NONE);
        let both = d.then(Delta {
            y: 5.0,
            scale: 2.0,
            ..Delta::NONE
        });
        assert_eq!((both.y, both.scale, both.alpha), (25.0, 4.0, 0.0));
    }
}
