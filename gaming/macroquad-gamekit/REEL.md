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
grade vhs mono=1 contrast=0.25 exposure=-0.15 vignette=0.35 scan=0.3 tape=0.55 bleed=0.4
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
| `tape` | Lines jitter sideways, grain crawls, and a tracking band rolls down every six seconds, tearing the picture |
| `bleed` | Red and blue pulled apart sideways |

The noise is a hash of the pixel and the frame's number, so a frame comes out the same every run.

### Fonts, images and styles

Fonts and images are declared before the first shot: `font NAME PATH`, `image NAME PATH`.
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

### Items

| Line | Places |
|---|---|
| `fill [@n] COLOUR` / `fill radial INNER OUTER` / `fill linear TOP … BOTTOM` | A full-frame colour or gradient |
| `image [@n] IMAGE [fit=cover\|contain] [at=X,Y size=WxH] [radius=PX]` | A named image over the frame, or cropped into a box with rounded corners |
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

## Running it

```sh
game --reel promo/clone-registry.reel --record out/clone-registry.mp4
game --reel promo/clone-registry.reel --shot .tmp/r.png --at 2,12,18.5   # r-2.png, r-12.png, …
```

Every still comes with the kit's text report, in the reel's pixels. The game's own text is muted
in it, because the game draws in its own points.

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

1. **The reel's own checks in the report:**
   - text off the frame, or under a platform's safe margins;
   - a glyph the font has not got. The app's Noto subset has no ✓ or ™, and both drew as
     nothing.
2. **A window preview** with scrubbing, a storybook story, and the CATALOG.
3. **Sound.** An `audio FILE` line, muxed in the same encode, once there is music.
4. **Stills for the carousel.** `SCRIPT.md` wants five 1080×1350 stills. That is a reel at that
   size, or `--shot` on this one with a crop.
