# macroquad-storybook

Every widget and composed screen in the kit, in one native window. The sidebar
lists stories by category; the stage renders the selected one. `--shot` captures
any story at an exact moment, so a UI change is reviewed by looking at it rather
than by deploying something.

```sh
cargo run -p macroquad-storybook
```

Click a story or use ←/→ and the number keys (the first nine); the sidebar
scrolls with the wheel or trackpad. A story added past the ninth is reached with
`--story` — and a story you cannot get on screen is a story you stop checking.

## Capture

```sh
cargo run -p macroquad-storybook -- --story app-shell/app-bar --shot .tmp/bar.png
cargo run -p macroquad-storybook -- --story gamekit/card-face --shot .tmp/face.png --at 1.25
```

A capture writes `<path>.png` **and** `<path>.txt`. The report is the second
check: it leads with Problems — text off the story's canvas, cut short, below
8 pt, overlapping — then lists every line drawn with its box and size. The image
is the review; the report catches what a glance misses.

| flag | what it does |
| --- | --- |
| `--story <slug or words>` | select a story: an exact slug, or the first matching category and name |
| `--shot <path>` | capture once, writing `<path>.png` and `<path>.txt` |
| `--at <seconds>` | capture when story time reaches this moment; the clock is fixed at 1/60 s, so it is exact |
| `--list` | print `slug<TAB>category · name<TAB>at=<seconds>\|-` for every story and exit |
| `--stage` | leave the sidebar out of the capture |
| `--no-chrome` | leave the sidebar, header and help line out: the story alone, at its declared canvas |
| `--check <dir>` | compare the capture with `<dir>/<os>.png`; exit 2 on a difference. `UPDATE_GOLDENS=1` writes the golden instead |
| `--tolerance <n>` | the per-channel difference a pixel may have (default 2) |
| `--fail-on-problems` | exit 2 when the capture's text report finds a problem |
| `--allow <kinds>` | with `--fail-on-problems`, problem kinds to ignore: `off,cut,small,overlap` |

In a browser, `?story=<words>` selects a story instead of `--story`.

There is no `--size`. A capture is the story's own canvas, declared by
`Story::canvas`, because a golden must not move when the storybook window does.
A story that is meant to show a cut (`app-shell/app-bar`, `gamekit/card-face`)
passes `--allow cut` in its test and still fails on everything else.

## Goldens

```sh
cargo test -p macroquad-storybook --test visual
```

`tests/visual.rs` holds four tests over the categories stable enough to pin
(`atoms`, `input`, `gamekit`, `page`, `app-shell`, `admin`):

1. every selected story has a golden;
2. every selected story matches it;
3. every selected story's text report is clean;
4. every golden belongs to a story that still exists.

Goldens live at `tests/goldens-<arch>/<slug>/<os>.png` — one set per
architecture and OS, because Metal and llvmpipe round differently, and llvmpipe
on arm64 and x86_64 differ too. A platform without its own set fails rather than
borrowing another's.

A responsive composition gets one story per breakpoint arm, not a story per
width: `app-shell/shell-phone` (390×844, bottom bar and phone insets) and
`app-shell/shell-desk` (1280×800, rail and no insets) share one body, and
`shell_layout` chooses between them from the canvas width.

Bless with:

```sh
UPDATE_GOLDENS=1 cargo test -p macroquad-storybook --test visual
```

That writes the current machine's set — arm64 on Apple Silicon, x86_64 on Intel.
Neither Linux set is written this way. `.stacks/linux-check` writes the aarch64
set in its arm64 container, and the x86_64 set in an amd64 `ubuntu:24.04`
container under Rosetta; its README has both commands, and the x86_64 path was
byte-identical to every committed golden when it was added. The x86_64 set is
also what the `Visual Goldens` workflow checks, on the runner whose Mesa is the
contract: a `bless_goldens` dispatch writes it and uploads `goldens-x86_64`.
A bless writes the whole set, so replace rather than merge. A bless silences
the picture check; it cannot silence the text report.

## The web build

`build.rs` stamps the wallet, EVM, platform and app-shell page scripts into
`web/` (gitignored), so the page loads the plugin the wasm was linked against.
The bundler then stages every `*.js` there:

```sh
cargo run -q -p mq-bundle -- --package macroquad-storybook \
    --wasm macroquad-storybook.wasm --title "macroquad storybook" \
    --dest .tmp/storybook-web --web ui/macroquad-storybook/web \
    --assets ui/macroquad-storybook/assets
```

Serve `.tmp/storybook-web` with any static server. The "wallet modal · real
wallets" stories talk to the extensions installed in that browser.

## Review corpus

`review/` is data, not a storybook feature: a fixed set of captures with known
defects and clean controls for comparing vision models and harnesses.
`review/README.md` is its layout, provenance and scoring; `review/RUBRIC.md` is
the fixed prompt. The runner lives in
`agent-playbook/integrations/review-bench`.
