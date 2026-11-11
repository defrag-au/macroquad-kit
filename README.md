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
| [`ui/macroquad-app/CATALOG.md`](ui/macroquad-app/CATALOG.md) | the app shell: routes, text entry, insets, pages, grids |
| [`ui/macroquad-storybook/README.md`](ui/macroquad-storybook/README.md) | stories, captures, the text report and the goldens |
| [`ui/macroquad-bundle/README.md`](ui/macroquad-bundle/README.md) | the web bundle: flags, steps, refusals |
| [`ui/macroquad-apk/README.md`](ui/macroquad-apk/README.md) | the APK: flags, the plan, the SDK, refusals |

The three catalogues are generated from each module's own `//!` header, so a
module without one fails its crate's test. The storybook and the two packaging
tools document their flags in a README, because the flags are the interface.

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

## Licensing

MIT OR Apache-2.0. The `macroquad` and `miniquad` forks keep their own
licences.
