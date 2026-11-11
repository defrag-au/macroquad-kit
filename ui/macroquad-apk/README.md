# macroquad-apk

Package a macroquad app as an Android APK. It takes miniquad's Java from the
version the app links, renders it and the app's manifest for one package, builds
the app as a `cdylib` per ABI, and runs javac → d8 → aapt2 → zip → zipalign →
apksigner. The APK is signed with the standard Android debug key, so a
reinstall updates in place and keeps app data.

The crate is a library with a one-line binary per consuming repo
(`tools/mq-apk`, whose `main` is `macroquad_apk::main()`): cargo runs only the
binaries of its own workspace, so the packager is pinned by the same revision as
the app it packages.

## Usage

```sh
cargo run -q -p mq-apk -- --package <android.package> --library <lib_name> \
    --crate <cargo-package> [--manifest <AndroidManifest.xml>] \
    [--icon <icon.png> --banner <banner.png> | --no-art] \
    [--abi <abi>]... [--out <file.apk>] [--dry-run]
```

| flag | what it is |
| --- | --- |
| `--package` | the app's Android package; the manifest must agree |
| `--library` | the cdylib's name as `System.loadLibrary` takes it: `x_mq`, giving `libx_mq.so` |
| `--crate` | the cargo package that holds the app's library target |
| `--manifest` | defaults to `<crate>/android/AndroidManifest.xml` |
| `--icon` | the launcher icon, xxxhdpi (192×192) |
| `--banner` | the TV banner, xhdpi (320×180) |
| `--no-art` | package no resources; the manifest must reference none |
| `--abi` | `arm64-v8a` or `armeabi-v7a` (their Rust target names also parse); repeatable, both by default |
| `--out` | defaults to `<target>/android/<package>.apk` |
| `--dry-run` | resolve everything and print every step; runs nothing and needs no SDK |

## The app crate

The package must hold the app in a **library** target. `macroquad-apk` runs
`cargo rustc --lib --crate-type cdylib`, and miniquad's Java loads the resulting
`lib<library>.so` and starts it through `quad_main`. The shape a consumer uses:

- `src/lib.rs` — the app, with the entry point exported for Android:

  ```rust
  pub fn run() {
      macroquad::Window::from_config(window_conf(), app());
  }

  #[cfg(target_os = "android")]
  #[unsafe(no_mangle)] // `#[no_mangle]` on edition 2021
  pub extern "C" fn quad_main() {
      run();
  }
  ```

  `app()` is the `async fn` that does the work — loads the fonts, then
  `macroquad_gamekit::app::run_on(Stage::Fluid, …).await`. `Window::from_config`
  is what `#[macroquad::main]` would have generated, as a call a library can
  make: the same `run()` serves native, wasm and Android, with `main.rs` and
  `quad_main` both just calling it.

- `src/main.rs` — `fn main() { my_app::run() }`, for native and web.
- `android/AndroidManifest.xml` — the app's own. `android/AndroidManifest.example.xml`
  beside this README is a minimal phone manifest to start from.
  `package="${package}"` is substituted (miniquad's `TARGET_PACKAGE_NAME` too);
  otherwise it must name the package `--package` gives. It needs `.MainActivity`,
  a launcher intent filter, GLES 2, and the `configChanges` miniquad expects. An
  app that uses `macroquad-app`'s `http`/`Fetch` also needs
  `android.permission.INTERNET` — without it the request fails with no useful
  error — and the example carries that line.
- `web/` — the page scripts `build.rs` stamps, for the web build.

`--library` is the library name (`field_guide`) and `--crate` the package
(`field-guide`); they are not the same string.

`macroquad-app` brings `ureq` on native targets, whose `rustls`/`ring` needs the
NDK's clang to build for `aarch64-linux-android`. Even `cargo check --target
aarch64-linux-android` has to run inside the android devshell.

The work directory is always `<target>/android/work`, and `--out` defaults to
`<target>/android/<package>.apk`, so a build writes into `target/` unless `--out`
says otherwise.

## What it resolves

- **The app crate** through `cargo metadata`, walked from `--crate` for
  `aarch64-linux-android` (`src/metadata.rs`).
- **miniquad's Java** from the miniquad version that graph reaches — never "the
  newest on disk". The JNI entry points in the `.so` and the `native`
  declarations in `QuadNative.java` are an unversioned contract, so a mismatch
  is an `UnsatisfiedLinkError` at launch. Two versions is an error.
- **The manifest**, rendered for `--package` (`src/manifest.rs`). The manifest is
  the app's own — its permissions, leanback features, activity and theme are not
  the packager's business. The one thing checked is the `package` attribute,
  because it decides what `.MainActivity` resolves to; `package="${package}"`
  and miniquad's `TARGET_PACKAGE_NAME` are substituted, and a disagreement is
  refused rather than rewritten.
- **miniquad's activity**, rendered from `MainActivity.java`'s `//%` markers
  (`src/activity.rs`). An unknown marker is an error: miniquad gaining one means
  this renderer has to learn what it means, and silently dropping it would
  compile a Java file with a feature missing.
- **The debug keystore** at `~/.android/debug.keystore`, created with
  `keytool` if it is missing.

## The plan

1. `cargo rustc -p <crate> --lib --release --target <abi> --crate-type cdylib`
   with `-C strip=symbols`, for each ABI, and the `.so` is copied into
   `work/apk/lib/<abi>/`.
2. The rendered manifest and `MainActivity.java` are written into `work/`.
3. `javac` (source and target 11) → `d8 --release --min-api <n>` → `aapt2
   compile` and `aapt2 link` → `zip` → `zipalign -p -f 4` → `apksigner sign` →
   `apksigner verify`.

Signing and verification are the last two steps, in that order, and the
verification's exit status is the build's.

## The SDK

A real build runs inside `defrag-nix`'s `android` shell, which brings the SDK,
the NDK and both Rust Android targets — platform 36, build-tools 36.0.0, NDK
29.0.14206865 and a minimum API of 30. It is an org repository with access
required (`https://github.com/defrag-au/defrag-nix`): from a checkout,
`nix develop .#android`; from elsewhere, `nix develop <checkout>#android`.
Without access, any SDK install that provides `aapt2`, `d8`, `zipalign`,
`apksigner` and both Android Rust targets will do — the variables below are the
whole contract.

The shell also exports the NDK's clang as each target's C compiler and linker:
`CC_aarch64_linux_android`, `CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER`, and the
`armv7` pair. That is what builds a C dependency in the tree (`ring`, under
rustls) for Android with no `.cargo/config`; the packager runs cargo without a
linker of its own.

From the environment:

| variable | default | what it is |
| --- | --- | --- |
| `ANDROID_HOME` / `ANDROID_SDK_ROOT` | none | required for a real build; a dry run prints a symbolic path instead |
| `ANDROID_PLATFORM` | `36` | the `android.jar` the Java is compiled against |
| `ANDROID_MIN_API` | `30` | the minimum API recorded in the dex and the manifest |

A non-numeric platform or min API is refused by name. A missing `android.jar`
for the pinned platform is refused before any step runs.

## What it refuses

- A package name that is not a Java package (it becomes a directory), or a
  library name that is not the name `System.loadLibrary` takes.
- `--icon` without `--banner`, `--banner` without `--icon`, art together with
  `--no-art`, or neither art nor `--no-art` — the choice is explicit because a
  manifest that references a missing resource fails at aapt2 link with a less
  useful message.
- A manifest whose package disagrees with `--package`.
- An unknown `//%` marker in miniquad's activity, or a missing
  `QuadNative.java`.
- An unset `ANDROID_HOME` or a missing `android.jar` outside a dry run.

## Output

```
✓ <out> — <n> KB, <package>, API <min>–<platform>
```

## The device half

This tool produces an APK and never needs a phone. Installing it, launching it
cold, reading `SAPP` and crash logs, and capturing the screen belong to
`android-shot` in `agent-playbook`, an org repository with access required
(`https://github.com/defrag-au/agent-playbook`): the tool is
`tools/android-shot/` and its procedure is `skills/android-shot/SKILL.md`,
published into `.skill-store/`. It owns `adb` and runs inside the same `android`
shell. Without access it has no hidden steps: `adb install -r`, `am start -n
<package>/.MainActivity`, `adb logcat` and `adb exec-out screencap -p`.
