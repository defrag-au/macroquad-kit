# Reels: a game's promo video, rendered by the game

A reel is a short video about a game. It shows the game's own screens, put inside phone or
browser frames, with captions, tags and stamps over them, and cuts between shots. The game renders
it itself, from a `.reel` file, and encodes it **once**. No step records an intermediate clip,
decodes it again in a browser, or hands it to another tool to compose.

## Status

Build step 1 is done:
- shots, cuts, the game, fills, images, phones;
- items and actions;
- `--record` and `--shot`.

The first reel is the Perps premint infomercial
(`cnft.dev-workers/workers/premint-status/app/promo/clone-registry.reel`). It was cut first in
HyperFrames, and that cut is the spec: every item below is one that film needs.

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

A copy change is an edit and a re-render: 14 s of 1080×1920 renders in about 4 s.

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

- `[name] SECONDS [cut=…]` starts a shot. Shots play in order, back to back.
- `cut=dip:0.35[:ink]` goes through a colour (black by default) and `cut=flash:0.35` through
  white. Either way the colour rises over the last 0.1 s of the shot before and falls away, eased,
  into this one.
- `game WxH px=N rate=R [skip=S] [continue]` runs the game in the shot:
  - at `W×H` points and `N` pixels a point;
  - its clock at `R` times true speed;
  - `skip=S` runs it `S` seconds on, undrawn, so the shot opens part way into a moment;
  - `continue` keeps the state of the previous shot's game instead of resetting it.

  The indented `set` lines are handed to `Game::setup`, as a capture plan's are.

### Items

| Line | Places |
|---|---|
| `fill [@n] COLOUR` / `fill radial INNER OUTER` / `fill linear TOP … BOTTOM` | A full-frame colour or gradient |
| `image [@n] IMAGE [fit=cover\|contain]` | A named image over the frame |
| `view [@n]` | The game over the whole frame |
| `phone [@n] at=X,Y width=W [label="…"] [accent=INK] [blank=WxH]` | A phone showing the game, or a blank screen of that shape |

Any item takes `in=@parent`. Its pose is then composed over its parent's (offsets add, scale and
alpha multiply), and it hides when its parent does.

### Actions

| Line | Does |
|---|---|
| `T enter @n [PRESET[:SECONDS]]` | Arrives from the preset's pose. Before it, the item is not shown |
| `T exit @n [PRESET[:SECONDS]]` | Leaves to the preset's pose, and is gone after |
| `T show @n` / `T hide @n` | At once |
| `T move @n SECONDS x= y= scale= rot= alpha= [ease=…] [yoyo=N]` | Moves by that much over the time; `yoyo` adds legs there and back |

- **Visibility.** An item shows from its shot's start, unless its first coming-or-going action is
  an `enter` or a `show`.
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

1. **Text, styles, taps and the signing sheet.** This adds:
   - `font` and `style` lines;
   - a `text` item through `RichText`, with `*emphasis*` in the style's `em` ink;
   - a `tap` item (a ring that swells and fades);
   - a `sheet` item clipped to its parent phone's screen;
   - the reel's checks in the report: text off the frame or under the safe margins.
2. **Grades.** A shader pass on an image or the game: `mono`, `contrast`, `exposure`,
   `vignette`, `scan`, `tape` and `bleed`, with the noise seeded by the frame. Then the opener's
   juggling phones (`phone … blank=`).
3. **A browser device for the desktop shot.** Then the rest of the storyboard, after which the
   HyperFrames project is removed.
4. **A window preview** with scrubbing, a storybook story, and the CATALOG.
5. **Sound later.** An `audio FILE` line, muxed in the same encode.
