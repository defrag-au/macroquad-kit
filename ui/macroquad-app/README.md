# macroquad-app

The shell for a macroquad app that is not a game: the page frame, how you move
through it, and how you type into it. Run the app on
`macroquad_gamekit::Stage::Fluid` (one stage unit per point) and lay it out each
frame with `shell_layout`. The crate brings what `macroquad-widgets`
deliberately refuses — routes, a real text field with the phone's keyboard, a
long clipped grid and a page that scrolls.

`CATALOG.md` is the module index. `examples/shell.rs` is the reference app: two
thousand cards, a search field, filters, a detail screen behind Back, the app
bar, the navigation and toasts. `examples/form.rs` is the form: three fields, a
validation error, a page of settings and a footer that stays disabled until the
handle is valid. Copy their shape.

## The frame

`shell_layout(size, insets, has_navigation)` returns the safe area, the app bar
band and its control row, the navigation (a bottom bar on a phone, a rail on
anything wider), the body and the gutter. The bars reach the screen's edges so
their colour fills the notch; their contents stay inside the safe area. While
the keyboard is open the bottom bar is not laid out and the body runs down to
the keyboard's top.

## Screens

- A **long list** is `List`: fixed-height rows, virtualised, scrolled with a
  thumb, and the host draws each row. `ArtGrid` is the same idea for square art
  in a grid. There is **no layout engine and no row widget**: a row's contents
  and its hit target are the host's, which is what keeps a settings screen and a
  trait list from growing a flexbox. `to_top` resets the scroll when a filter
  changes; a count of zero draws no rows and has no scroll range, so the host
  draws its `Message` in the area.
- A **detail or settings screen** that scrolls as a whole is `Page`: `begin`
  moves it and says where to lay out; `end` records where the layout finished,
  which is the scroll's end next frame.
- **Empty, loading and failed** states are `macroquad_widgets::Message` — from a
  `Load`/`Fetch`, or from a local filter that matched nothing. `show`/`show_in`
  return whether its action was tapped, so a "Clear search" action is one call
  plus the filter reset. Do not draw your own paragraph of apology.
- **Routing**: name your screens with a `Route`, parse and write the hash, poll
  once a frame, and show what `poll` reports. The app bar's arrow is
  `Router::up(parent)` when back stays inside the app, and `Router::back()` for
  the browser's back and Escape; the example shows both. The router moves
  screens, not their state: field values live on your `Game`, so back and
  forward keep them, but a reload loses anything the route does not carry — put
  what must survive in the hash.

## Data

`Load` and `Fetch` decode a reply into a type you own, so that type derives
`serde::Deserialize`; a client using either carries `serde` with `derive`.
`Load::Failed`'s reason is the transport's, not a sentence — offline it reads
`Connection Failed: Connect error: Connection refused (os error 111)` — so map it
before showing it, and let `Message` show words you chose.

## Typing

One `Keyboard` per app, wrapped around the frame's fields with
`keyboard.begin(frame)` and `keyboard.end()`. `TextField::show` takes the
painter, the keyboard, its rect, a `FieldKind` (which decides the phone's
keyboard and its Search key) and a placeholder. On the web the canvas cannot
take text, so while a field is edited a real `<input>` sits over it — the page
half is the app-shell page script, stamped as `gamekit-app-shell.js`
(`macroquad_gamekit::web::PageScript::AppShell`).

Every field draws its own × once it has a value, and a `Search` field adds the
lens; a clear is a `FieldReply.changed` like typing is. There is no error state:
draw the invalid stroke and the message under the field yourself, and pass
`enabled = false` to `Painter::button` until it validates.

The page's `<input>` draws in the browser's font unless the app names its own.
Call `Keyboard::input_font("Noto Sans", "assets/NotoSans-Bold.ttf")` once: the
page loads a `FontFace` from that URL, so the text does not change weight when a
field takes the focus. `mq-bundle --assets <dir>` stages the file at `assets/`
beside the page, and the URL is relative, so `--base` works too. The page also
clips the input to `Keyboard::clip(rect)` — the body a screen draws its fields
in — so a field scrolled under the app bar is cut exactly as the canvas cuts it,
not left floating over the chrome.

## Drawing

Draw text through the kit's `Painter` and `macroquad-text`, at `frame.px`, and
only when `frame.text` is true (it is false while the window settles). Text
drawn too early can vanish for a frame, before the glyph atlas has warmed; the
preloader covers the settle in these examples, and a client with a slow first
layout should gate its text. `Painter` resolves sizes through the theme ramp, so
a point size is never written down.

## Captures

An app uses the gamekit harness flags, not the storybook's:

```sh
cargo run -p my-app -- --shot .tmp/phone.png --size 390x844
cargo run -p my-app -- --route /item/41 --shot .tmp/detail.png --size 390x844
cargo run -p my-app -- --shot .tmp/notch.png --size 390x844 --insets 47,0,34,0
cargo run -p my-app -- --plan .tmp/plan.txt --size 390x844   # several states, one launch
```

- `--size` is the window and the capture; the capture draws into its own target.
- `--insets` is `top,right,bottom,left[,keyboard]`, in points. The keyboard
  value only reserves the band. A capture can still show a field being edited:
  `Keyboard::focus(id)` focuses it as a tap would, so a `set focus handle` line
  in `Game::setup` gives the caret and the keyboard-open layout. The IME is the
  one thing a capture cannot show.
- The `.txt` beside the image is the second check. `--fail-on-problems` exits 2
  when the report finds a problem (text off the surface, cut short, below 8 pt,
  overlapping); `--allow off,cut,small,overlap` excludes a class, so a screen
  that exists to show a cut passes `cut` and still fails on everything else.
  Both flags apply to `--plan` too, to every state it captures.
- The report checks text against the capture's bounds, not against what covers
  it: a row under `with_clip` counts as on-surface, and a row hidden behind the
  keyboard band is not flagged. Read the image for what is occluded.
- A plan cannot tap, drag or type. A section reaches its state through
  `Game::setup` lines — `set search 12` in `examples/shell.rs` — and the app
  decides what each line means, so `set route /prefs` and `set scroll bottom`
  are your own conventions, not the harness's. The same `Game` runs every
  section, so state carries over unless `setup` resets it.
- `window_conf` must be `harness::Args::read().window_conf(conf)`, or a capture
  opens a window (and panics with no display). `examples/shell.rs` is the shape.

The full flag list is the `//!` header of
`gaming/macroquad-gamekit/src/harness.rs`.

## Fonts and web

The kit ships no font for consumers. The examples use Noto Sans Bold and
JetBrains Mono (both OFL) from `ui/macroquad-storybook/assets/`; copy the faces
you want into your crate and load them with `load_font`, so the glyph gate can
warm them.

The page needs the shell's plugin. `build.rs` can stamp it through the gamekit,
which does not compile this crate (or its native HTTP stack) for the host:

```rust
use macroquad_gamekit::web::{PageScript, stamp_scripts};
stamp_scripts(std::path::Path::new("web"), &[PageScript::AppShell])?;
```

`macroquad_app::PLUGIN_JS` is the same text, for a build script that writes the
file itself. The bundle stages that directory with `--web`;
`ui/macroquad-bundle/README.md` has the rest.

Stamping only writes, and the removal of an old name runs only when cargo
re-runs the build script: a kit rev bump does, because the gamekit is a
build-dependency, but a build with nothing changed does not. So a `web/` built
before the page script moved into the gamekit can still hold `app.js`. The bundle
stages every `*.js` there, so the page loads the page script twice — and
`mq-bundle` cannot tell, because both copies provide the imports. If your
`build.rs` removes it, touch `build.rs` and run `mq-bundle` again to make that
run — a bare `cargo build --target wasm32-unknown-unknown` runs the script and
then fails to link without `--allow-undefined`
(`ui/macroquad-bundle/README.md` says why). Otherwise delete `app.js` by hand;
`ui/macroquad-wallet-link/build.rs` is the worked example.

## Android

The app lives in a library target for `mq-apk`, with `quad_main` exported from
it, and carries its own `android/AndroidManifest.xml`.
`ui/macroquad-apk/README.md` has the shape, and
`ui/macroquad-apk/android/AndroidManifest.example.xml` is a minimal phone
manifest to copy. An app that uses `http` or `Fetch` needs
`android.permission.INTERNET` in it; the example carries the line.
