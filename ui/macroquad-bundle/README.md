# macroquad-bundle

Build a macroquad client for `wasm32-unknown-unknown` and stage it as a static
web bundle: the wasm, the miniquad JavaScript that matches the version it
linked, the crate's own plugin scripts, `index.html` and a web app manifest. It
also checks that every function the wasm imports is provided by a staged
script, so a missing plugin is a build failure rather than a silent game.

The crate is a library with a one-line binary per consuming repo: cargo runs
only the binaries of its own workspace, so a repo carries `tools/mq-bundle`
whose `main` is `macroquad_bundle::main()`. That is what makes
`cargo run -q -p mq-bundle -- …` work in CI with nothing but cargo.

## Usage

```sh
cargo run -q -p mq-bundle -- --package <crate> --wasm <bin>.wasm \
    --title "<Title>" --dest <dir> [--web <dir>] [--base /path/] \
    [--shell <template.html>] [--no-build]

cargo run -q -p mq-bundle -- check <bundle-dir> <file.wasm>
```

| flag | what it is |
| --- | --- |
| `--package` | the cargo package to build (`-p`) |
| `--wasm` | the bin target's file name, which is also the name staged in `--dest` |
| `--title` | the page title and the web app manifest's name |
| `--dest` | the output directory |
| `--web` | a directory of extra `*.js` to copy — the crate's stamped page scripts |
| `--base` | a path prefix for every URL, such as `/pump/`; a trailing slash is added |
| `--shell` | an `index.html` template with `__TITLE__`, `__SCRIPTS__` and `__WASM__` |
| `--no-build` | stage a wasm that is already built |
| `check <dir> <wasm>` | verify a bundle staged some other way |

## What a run does

1. **Checks the macroquad `[patch]`** before building. A gamekit client on a
   crates.io macroquad at or below 0.4.16 is refused, naming the lines to copy
   (`MACROQUAD_PATCH` in `src/lib.rs`). Any other macroquad client gets a
   warning. A `[patch]` applies only in the workspace that builds, and without
   it one run in twelve doubles the glyph atlas.
2. **Builds** the release wasm with `-C link-arg=--allow-undefined` in
   `RUSTFLAGS`. rustc 1.96 stopped passing it to wasm-ld, and miniquad and
   sapp-jsutils still declare bare `extern "C"` imports the page provides —
   without it they do not link, and `cargo check` passes regardless.
3. **Resolves** `gl.js`, `sapp_jsutils.js` and `quad-net.js` from the versions
   *this package* links, through `cargo metadata` (`src/metadata.rs`) — never a
   CDN or a copy committed beside the code.
4. **Clears** the `.js`, `.wasm` and `index.html` a previous run left in
   `--dest`, and nothing else, so a plugin no longer needed is not served
   beside a wasm that does not import it.
5. **Stages** `gl.js`, `sapp_jsutils.js` when linked, `quad-net.js` when the
   wasm imports a socket or HTTP function, then every `*.js` in `--web`. The
   crate's own page scripts are stamped into `--web` by its `build.rs`: the app
   shell through `macroquad_gamekit::web::stamp_scripts(PageScript::AppShell)`,
   miniquad plugins through `stamp`. Nothing is copied from a plugin crate by
   hand.
6. **Renders** `index.html` from the shared shell (`src/page.rs`, `shell.html`)
   with the wasm content-hashed into `load()`, so a proxy or browser cannot keep
   a stale build alive, and writes `manifest.webmanifest`.
7. **Checks** that every function the wasm imports is provided by a staged
   script (`src/imports.rs`). gl.js stubs a missing import instead of refusing
   to load, so this is where a missing plugin becomes an error naming the
   functions.

## What it refuses

- A gamekit client built on a crates.io macroquad that still has the
  glyph-atlas bug, naming the `[patch]` to copy. A non-gamekit client is warned.
- A `--web` directory that carries its own `gl.js`, `sapp_jsutils.js` or
  `quad-net.js`; the bundler stages those, and a second copy would drift.
- A wasm that imports a function no staged script provides, listing the
  functions and the scripts that were staged.
- A crate reached at two versions in the resolve graph — a page can load only
  one gl.js.
- A wasm that is not where `--wasm` says after the build.

## Output

```
✓ <title> → <dest> (<n> KB wasm, cache key <hash>, <m> imports provided by <scripts>)
```

The dest holds `index.html`, `manifest.webmanifest`, the wasm, and the staged
scripts. A failure exits 1 with `✗ <reason>`; `--help` prints the usage.

## The two things worth remembering

- **The JS comes from the versions the package links.** The plugin protocol is
  untyped and unversioned, so a gl.js from one miniquad beside a wasm built
  against another is a blank canvas with no error.
- **`--allow-undefined` is required.** It is appended to the build, not left to
  the caller, because a build without it fails at link time in a way that reads
  like a missing page script.
