# macroquad-kit

The macroquad layer for defrag-au apps and games: text, widgets, the game-feel
kit, the app shell, the web/APK bundlers, the storybook, and the Cardano edges
that draw with them.

The kit **consumes** `shared-crates` for the pure crates it needs
(`ui-theme`, `chains`, `perf-probe`, `cnft-holdings`, `image-core`,
`wallet-miniquad`, `chain-heartbeat`, `discord-auth`, `arcade-core`,
`scenegraph`, …). The direction is one-way:

```
cnft.dev-workers ─┐
augminted-bots ───┼─► macroquad-kit ─► shared-crates
tile5-macroquad ──┘
```

**No crate in `shared-crates` may depend on a crate in this repo.** That is the
invariant the extraction preserves; if it is ever violated, a rev bump starts
ping-ponging between the two repositories.

## Where to look

Each crate reached for cold has an index beside it:

| Read | For |
| --- | --- |
| [`ui/macroquad-widgets/CATALOG.md`](ui/macroquad-widgets/CATALOG.md) | the stateless widgets: atoms, molecules, bounded views |
| [`gaming/macroquad-gamekit/CATALOG.md`](gaming/macroquad-gamekit/CATALOG.md) | stateful game modules that hold across frames |
| [`gaming/macroquad-gamekit/ANIMATION.md`](gaming/macroquad-gamekit/ANIMATION.md) | sprites, gaits and the motion rubric |
| [`gaming/macroquad-gamekit/CARD-FACES.md`](gaming/macroquad-gamekit/CARD-FACES.md) | card faces: the visible strip, layers, baking |
| [`ui/macroquad-app/README.md`](ui/macroquad-app/README.md) | building an app: the shell, screens, a list, empty states, back, captures (module index in `CATALOG.md`) |
| [`gaming/macroquad-gamekit/src/harness.rs`](gaming/macroquad-gamekit/src/harness.rs) | the capture loop every client runs: `--shot`, `--size`, `--plan`, the text report, headless |
| [`ui/macroquad-storybook/README.md`](ui/macroquad-storybook/README.md) | the storybook's own stories, capture flags and goldens (not an app's) |
| [`ui/macroquad-bundle/README.md`](ui/macroquad-bundle/README.md) | the web bundle: flags, steps, refusals |
| [`ui/macroquad-apk/README.md`](ui/macroquad-apk/README.md) | the APK: app shape, flags, the plan, the SDK, refusals |

The three catalogues are generated from each module's own `//!` header, so a
module without one fails its crate's test. The storybook and the two packaging
tools document their flags in a README; a client's capture flags are the harness
module's header.

## Build and test

The toolchain comes from the flake. Run `direnv allow .` once, then `cargo` is on
`PATH` in an interactive shell. A shell an editor or an agent spawned sources no
rc file, so reach the same devshell explicitly:

```sh
direnv exec . cargo test --workspace
direnv exec . cargo clippy --workspace --all-targets -- -D warnings
direnv exec . cargo check --target wasm32-unknown-unknown -p macroquad-widgets --all-features
```

A minimal app and game ship as examples:

```sh
cargo run -p macroquad-app --example shell
cargo run -p macroquad-gamekit --example board
```

The storybook runs natively:

```sh
cargo run -p macroquad-storybook
```

and its visual goldens:

```sh
cargo test -p macroquad-storybook --test visual -- --test-threads=2
```

The web bundle is staged by `mq-bundle`:

```sh
cargo run -q -p mq-bundle -- --package <crate> --wasm <bin>.wasm \
    --title "<Title>" --dest <dir> --web <crate>/web
```

## The two patch lines

`macroquad` 0.4.16 doubles its glyph atlas at random, and the headless capture
path needs `miniquad`'s `Conf::headless` fork. A `[patch]` only applies in the
workspace that builds, so **every consumer of the kit must carry the same two
lines** as this repo's `Cargo.toml`:

```toml
[patch.crates-io]
macroquad = { git = "https://github.com/not-fl3/macroquad", rev = "8d602d3f8ebb8a3a582ee15d92983bbb81ee3ef6" }
miniquad = { git = "https://github.com/defrag-au/miniquad", rev = "59178c9744c6881e8d600d787e5a4284ab5ccbdc" }
```

`mq-bundle` refuses to stage a client without the macroquad patch. Grep
`defrag-au/miniquad` in `cnft.dev-workers`, `augminted-bots` and
`tile5-macroquad` when either rev changes.

## The shared-crates rev

The kit pins one `shared-crates` rev in `[workspace.dependencies]`. Every
consuming repo must pin the **same** rev, or Cargo can resolve two sources for
the same package. Bump both together. To develop against a sibling checkout,
uncomment the whole `[patch."https://github.com/defrag-au/shared-crates"]`
block at the bottom of `Cargo.toml` — as a unit, never partially.

## Consuming the kit

A new app, game or client is its own workspace and takes the kit by git rev.
Every kit crate it uses points at the same rev:

```toml
[workspace.dependencies]
macroquad-app = { git = "https://github.com/defrag-au/macroquad-kit", rev = "<full sha>" }
macroquad-gamekit = { git = "https://github.com/defrag-au/macroquad-kit", rev = "<full sha>" }
macroquad-widgets = { git = "https://github.com/defrag-au/macroquad-kit", rev = "<full sha>" }
macroquad-bundle = { git = "https://github.com/defrag-au/macroquad-kit", rev = "<full sha>" }
macroquad-apk = { git = "https://github.com/defrag-au/macroquad-kit", rev = "<full sha>" }
macroquad = "0.4"
```

The last two are what `tools/mq-bundle` and `tools/mq-apk` take with
`workspace = true`; the block is the one place a rev is written, and every line
carries the same one.

The same root `Cargo.toml` carries **both** `[patch]` lines above. They are not
only for the gamekit: the miniquad one is what gives every client the headless
capture path.

It also carries:

- **`tools/mq-bundle`** — the binary cargo will not run from another workspace:

  ```toml
  [dependencies]
  macroquad-bundle = { workspace = true }
  ```

  ```rust
  fn main() -> std::process::ExitCode {
      macroquad_bundle::main()
  }
  ```

- **`tools/mq-apk`** — the same, over `macroquad-apk`.
- **`build.rs`** — stamp the plugins the client uses into `web/`. The app shell
  is `PageScript::AppShell` through `macroquad_gamekit::web::stamp_scripts`, so
  the build script does not compile `macroquad-app` for the host; the gamekit's
  are `Plugin::Audio`, `Plugin::Gamepads` and so on, through
  `macroquad_gamekit::web::stamp`.
- **fonts** — the kit ships none for consumers. Its examples use Noto Sans Bold
  and JetBrains Mono (both OFL) from `ui/macroquad-storybook/assets/`; copy the
  faces you want into your crate and load them with the kit's `load_font`.

For an APK the app lives in a **library** target: `src/lib.rs` holds the app and
exports miniquad's entry point, and `src/main.rs` calls into it for native and
web. `macroquad-apk` builds the library as a `cdylib`, which is what the Android
activity loads. `ui/macroquad-apk/README.md` has the shape, and
`ui/macroquad-apk/android/AndroidManifest.example.xml` is the manifest to copy.

The repository is private, so cargo's git fetch needs credentials: a credential
helper (`credential.https://github.com.helper = !gh auth git-credential` in
`~/.gitconfig`), or `CARGO_NET_GIT_FETCH_WITH_CLI=true` so cargo uses the system
git and its SSH key.

If the client also takes a pure crate from `shared-crates` directly, rather than
through a kit re-export, pin the rev the kit's `Cargo.toml` names. Two different
revs resolve two sources for the same package.

## Licensing

MIT OR Apache-2.0. The `macroquad` and `miniquad` forks keep their own
licences.
