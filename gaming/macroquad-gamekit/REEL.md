# Reels: a game's promo video, rendered by the game

A reel is a short video about a game. It shows the game's own screens, put inside phone or
browser frames, with captions, tags and stamps over them, and cuts between shots. The game renders
it itself, from a `.reel` file, and encodes it **once**. No step records an intermediate clip,
decodes it again in a browser, or hands it to another tool to compose.

## Status

In use. The first reel is the Perps premint infomercial
(`cnft.dev-workers/workers/premint-status/app/promo/clone-registry.reel`): 84 s, rendered in
about 48 s.

That film was cut first in HyperFrames, and the reel replaced it. Every item below is one that
film needed.

## Why in the game and not after it

The game already draws deterministic frames on a fixed clock. A reel drives it through a narrow
`Player` (`app.rs`), which does four things:
- resets the game;
- hands it a shot's `set` lines;
- gives it a viewport;
- asks for frames, each drawn into the player's own target at that viewport's size, whatever the
  window is.

The reel composes that frame into its own and pipes the result to ffmpeg:

```text
shot's set lines ──► Player ──► game frame, in its own target at the shot's viewport
                                     │
scene(reel, t) ──► fills, images, the game frame in its phone, text … each posed
                                     │
                                     ▼
                           one read-back ─► ffmpeg ─► reel.mp4
```

What this rules out:
- the round trip through MP4: encode, decode back to PNG frames, encode again;
- extraction timeouts;
- a browser in the render path;
- a second copy of the copy text.

A copy change is an edit and a re-render: 60 s of 1080×1920 renders in about 21 s.

## Items and actions

A shot **places** items. **Actions** then move them.

- **Items.** An item is placed once: where it is and what it looks like. Name it with `@` to act
  on it.
- **Actions.** An action is a line that starts with a time, in seconds into the shot. It names an
  item and says what happens to it from then on.
- **Poses.** An item's pose at a moment is its placement, with every action due by then folded
  over it in time order. Nothing is stepped or accumulated. Any frame can be computed on its own,
  so a still at 18.5 s is exactly the video's frame at 18.5 s.

```text
reel 1080x1920 30                                   # size and frame rate
ink gold #f5c542                                    # named colours
ink ground #070c09
image lab app/static/scene/lab-2048.jpg             # named images, before the first shot

[step1] 12 cut=dip:0.35:ground                      # a shot: name, length, how it cuts in
fill radial #12301a ground                          # items, back to front
game 390x844 px=3 rate=0.6                          # the game in this shot, and its speed
  set identity app/fixtures/identity-registered.json
phone @ph at=266,256 width=548 label="MetaMask's browser"

0.3  move @ph 0.45 y=-240 rot=-22 ease=sine-in-out yoyo=5   # actions: TIME VERB @item …
11.0 exit @ph drop
```

### Shots

- `[name] SECONDS [cut=…] [grade=NAME]` starts a shot. Shots play in order, back to back.
- `grade=` lays a named grade over the whole shot once it is drawn: one shader pass, so its items
  and the game's screen take the same look.
- `cut=dip:0.35[:ink]` goes through a colour (black by default) and `cut=flash:0.35` through
  white. Either way the colour rises over the last 0.1 s of the shot before and falls away, eased,
  into this one.
- `game WxH px=N rate=R [skip=S] [continue]` runs the game in the shot:
  - at `W×H` points and `N` pixels a point;
  - its clock at `R` times true speed;
  - `skip=S` runs it `S` seconds on, undrawn, so the shot opens part way into a moment;
  - `continue` keeps the state of the previous shot's game instead of resetting it.

  The indented `set` lines are handed to `Game::setup`, as a capture plan's are. Name the game
  (`game @app …`) to change it mid-shot with a `set` action: `8 set @app identity
  registered.json` does what a tap on its screen would have done.

### Grades

```text
grade vhs mono=1 contrast=0.25 exposure=-0.15 vignette=0.18 scan=0.3 tape=0.55 track=0.25 bleed=0.15
```

The look of a dubbed infomercial off a worn tape. Each control is 0 when not given, which leaves
the frame alone.

| Control | Effect |
|---|---|
| `mono` | Black and white |
| `contrast` | Stretched about mid-grey |
| `exposure` | In stops |
| `vignette` | Darkened corners |
| `scan` | Scanlines four pixels apart |
| `tape` | The tape's texture: lines jitter sideways and grain crawls |
| `track` | A tracking band rolls down every six seconds, tearing the lines it crosses sideways (up to 24 px at 1) |
| `bleed` | Red and blue pulled apart sideways |

The noise is a hash of the pixel and the frame's number, so a frame comes out the same every run.

### Fonts, images, sounds and styles

Fonts, images, sounds and shaders are declared before the first shot: `font NAME PATH`,
`image NAME PATH`, `sound NAME PATH`, `shader NAME PATH`.
Images may be PNG or JPEG. macroquad's own decoder reads PNG only, so the kit turns on JPEG
for the same `image` crate, natively.

A style sets text in one of them:

```text
style NAME FONT SIZE INK [left|centre|right] [top|middle] [em=INK] [shadow=INK] [wrap=PX]
                         [gap=PX] [plate=INK] [pad=X,Y] [radius=PX] [border=PX]
```

- **Alignment.** It says where a text item's `at` is.
  - Across: the left end, middle or right end of its lines.
  - Up and down: `top` (the default) is the top of the first line; `middle` is the middle of
    the text's ink.

  The line box leaves room below for descenders that capitals don't have, so `middle` is what
  lines up a tag and the label beside it: both `middle`, at one y.
- **Emphasis.** `*stars*` around words set them in the `em` ink.
- **Plates and borders.** A plate makes a tag, and a border in the style's ink makes a stamp.
  Their padding is measured from the ink, so "STEP 4" sits centred in its plate.

### Shaders

An image may be drawn through a fragment shader: declare it, then name it on the image.

```text
shader tube app/promo/tube.frag
image art at=-120,0 size=1320x1350 shader=tube
```

The shader is written against the reel's standard pair of varyings and one uniform:

| In the fragment shader | Is |
|---|---|
| `sampler2D Texture` | The picture. |
| `varying vec2 uv` | The picture's **own** uv, 0..1 across the whole file — not across the box it is drawn in. A shader measured against the painting therefore holds wherever the frame crops it. |
| `varying lowp vec4 color` | The item's tint, its alpha in `color.a`; `gl_FragColor = … * color` is how it fades. |
| `uniform float Time` | Seconds from the reel's start. Never the wall clock, so a still at 6.4 s is the frame the video has at 6.4 s. |

The image covers its box (as `fit=cover` does) and is drawn square-cornered, so `radius=` is
refused beside `shader=`: say so rather than draw a corner the shader cannot round. A shader
that fails to compile fails the render, naming its path — a reel with a still effect that nobody
noticed is the thing to avoid. The source is `#version 100` GLSL, like the kit's own, so the same
file draws natively and in a browser.

`macroquad` composes the reel's model matrix above it, so the shader draws where the item was
placed and moves with it.

### Items

| Line | Places |
|---|---|
| `fill [@n] COLOUR` / `fill radial INNER OUTER` / `fill linear TOP … BOTTOM` | A full-frame colour or gradient |
| `image [@n] IMAGE [fit=cover\|contain] [at=X,Y size=WxH] [radius=PX] [shader=NAME]` | A named image over the frame, or cropped into a box with rounded corners — or drawn through a shader (see "Shaders") |
| `rect [@n] at=X,Y size=WxH [fill=INK] [stroke=INK] [border=PX] [radius=PX]` | A rectangle: a full-frame banner, a border around a card |
| `view [@n]` | The game over the whole frame |
| `browser [@n] at=X,Y width=W [url="…" bar=STYLE] [chips="EVM ADA" chip=STYLE] [accent=INK]` | A desktop browser showing the game: a toolbar with the address and extension chips, over a page of the game's shape |
| `phone [@n] at=X,Y width=W [label="…" chip=STYLE] [accent=INK] [blank=WxH]` | A phone showing the game, or a blank screen of that shape. A label sits on its top edge as a chip |
| `text [@n] STYLE at=X,Y "…"` | Text in a style. `\n` breaks a line, and a blank line between paragraphs is kept |
| `tap @n at=X,Y [size=PX] [ink=INK]` | A finger's ring. It shows only while a `pulse` plays it |
| `sheet @n in=@phone [fill=INK] [scrim=0.6]` | A sheet up from the bottom of the phone's screen, cut to it, over a scrim that darkens as it arrives |

A sheet's rows are indented under it:
- `line STYLE "…"`
- `box STYLE "…" [fill=INK]`, the message being signed;
- `buttons STYLE "Cancel" "Sign"`. Each is outlined and the last is filled.

Any item takes three placement options:

- **`rot=DEG`**: it is turned as placed, about its centre.
- **`within=X,Y,WxH`**: it is cut to that window in the frame, wherever its pose takes it. The
  fine print scrolls past a window by moving a text item `within=` one.
- **`in=@parent`**: it is placed in the frame like any item. It is then carried by its parent:
  moved, turned and scaled about the parent's centre, faded with it, and hidden when the parent
  is. Text placed on a phone that is being juggled turns with the phone.

### Actions

| Line | Does |
|---|---|
| `T enter @n [PRESET[:SECONDS]]` | Arrives from the preset's pose. Before it, the item is not shown |
| `T exit @n [PRESET[:SECONDS]]` | Leaves to the preset's pose, and is gone after |
| `T show @n` / `T hide @n` | At once |
| `T move @n SECONDS x= y= scale= rot= alpha= [ease=…] [yoyo=N]` | Moves by that much over the time; `yoyo` adds legs there and back |
| `T pulse @n [SECONDS]` | Shows, swells from 0.4× to 1.5× and fades, then is gone: a tap |
| `T sound NAME [volume=0.6]` | Starts a named sound at that moment; see "Sound" |
| `T set @game LINE` | Hands the game a condition line, mid-shot |

- **Visibility.** An item shows from its shot's start, unless its first coming-or-going action is
  an `enter`, a `show` or a `pulse`.
- **The game's own state.** The game is the one stateful thing in a reel. Frames run forward, so a
  `set` action is handed over once, on the first frame at or after its time. A still is taken
  by running the reel up to it, so it sees the same game.
- **Moves.** Moves add up.
- **Presets.**

  | Preset | Pose it comes from or goes to |
  |---|---|
  | `cut` | None: the item is simply there, or not |
  | `fade` | Fades in or out |
  | `rise` | Fades while moving 20 px |
  | `slam` | Fades, at 2.4× scale |
  | `pop` | Fades, at 0.6× scale |
  | `slide` | One item-height down |
  | `drop` | Most of a frame down, turning 140° |

  Entering eases with the preset's own curve; leaving always gathers speed.
- **Eases.** Eases are the kit's `Ease` names, hyphenated: `sine-in-out`, `back-out`.

### Sound

A sound is declared before the first shot, then either cued at a time or hung on an action:

```text
sound bed app/audio/bed.mp3
sound whoosh app/audio/whoosh.wav

[title] 8
0.4 sound whoosh volume=0.6          # a cue: nothing moves
0.3 enter @title slam sound=whoosh   # an effect: starts when the action does
```

- **`sound=NAME` on an action** starts the sound when the action starts, so retiming the action
  takes its sound with it. Every action but `set` takes one; `volume=` rides with it.
- **A cue line** (`T sound NAME`) is for a sound with nothing to move: a bed, a room tone.
- **Both are in shot time**, like an action: moving a shot moves its sounds with it.
- **Every sound is a file**, so a bed is a cue at `0` with a file as long as the reel; a shorter
  bed simply stops. Nothing loops.
- **Levels add up** — the mix is not normalised — so a cue over a bed wants less than 1.
- **It is mixed in the same encode** as the picture: no second pass and no mux, and the sound is
  AAC in the `.mp4`.
- **The preview is silent.** Sound is in the recorded file, not in the window.

## Running it

```sh
game --reel promo/clone-registry.reel                               # the preview: a window
game --reel promo/clone-registry.reel --record out/clone-registry.mp4
game --reel promo/clone-registry.reel --shot .tmp/r.png --at 2,12,18.5   # r-2.png, r-12.png, …
game --reel promo/carousel.reel --shot out/card.png --at problem,sealed  # card-problem.png, …
```

`--quality share` encodes a copy for posting: CRF 26 instead of the master's near-lossless 16.
The infomercial is 141 MB as a master and 9.1 MB shared, under Discord's 10 MB limit. UI text
stays crisp and the tape grain is a little smoother. Keep the master for cutting and
re-encoding.

`--at` takes seconds, or a shot's name. A name takes that shot's last frame, where everything in
it has come to rest. A set of cards is a reel with one short shot per card, taken by name.

### The preview

With neither `--record` nor `--shot`, the reel plays in a window in real time.

| Key | Does |
|---|---|
| Space | Play or pause |
| ← / → | Seek one second (five with Shift) |
| `,` / `.` | Step one frame |
| Home | Back to the start |
| S | Toggle the safe area's outline. Text outside it is boxed in red as it plays |
| R | Read the file again and keep the place |
| Q / Esc | Quit |

`R` is the edit loop: change the file, save, press R. A mistake in the file is shown at the top
of the window, and the last good reel keeps playing.

Seeking puts the game where the reel would have it. It replays the game, undrawn, from the first
shot of the run of `continue` shots the moment is in.

### Checks

- **Missing glyphs.** Before anything is drawn, every string the reel sets is checked against its
  style's font: text, labels, a browser's address and chips, and sheets' rows. A missing
  character stops a recording or a still, naming its line:

  ```text
  clone-registry.reel:72: font `noto` has no ✓ (U+2713), which would draw as nothing
  ```

  The preview shows the same message instead of stopping.
- **The safe area.** Declare `safe top=… right=… bottom=… left=…` in the reel. Text that rests
  outside it is then reported:
  - in each still's report, under "Reel checks";
  - after a recording, as one line per item with the spans of time it was out;
  - in the preview, boxed in red.

  With `--fail-on-problems`, any of these exits 2.
  - Text on its way in or out isn't held to the area while an action of its own or a parent's is
    under way, because it passes through places it does not stay.
  - `safe=ignore` exempts an item meant to leave the frame.

Every still also comes with the kit's text report, in the reel's pixels. The game's own text is
muted in it, because the game draws in its own points. A reel's text is written, not cut to fit,
so a trailing "…" in it is not reported as cut short.

A reel renders offline, so its text and the game's are drawn with the glyph gate open. Every
glyph is cached as it is first drawn, and no caption is held back a frame.

Gated text depends on the game's frame to flush the cache. A shot with no game would otherwise
never show its text at all; the opener found that.

## What is pure and what draws

- **`reel::file`** parses and checks a reel. It is pure. Every mistake names its line:
  - an action on an item that does not exist;
  - an action after its shot ends;
  - a phone showing a game in a shot that has none;
  - an unknown preset or ease, which lists the ones there are.
- **`reel::timeline`** is `scene(reel, t)`: the shot, each item's pose, and the cut's veil. It is
  pure.
- **`reel::draw`** draws a scene. Each item is drawn under the model matrix of its pose: moved,
  then turned and scaled about its own centre.
- **`reel::run`** is `--reel`. It drives the game only through `Player`. Nothing in the frame
  loop is opened up for it.

## Next

1. **An `include` line,** if a second reel for the same game keeps copying the first's inks,
   fonts and styles. `carousel.reel` beside the infomercial copies about 30 lines.
