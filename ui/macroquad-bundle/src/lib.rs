//! `macroquad-bundle` — build a macroquad client for the browser and stage it
//! as a static bundle, the same way in every repo.
//!
//! ```sh
//! cargo run -p mq-bundle -- --package <crate> --wasm <bin>.wasm \
//!     --title "<Title>" --dest <dir> [--web <dir>] [--assets <dir>] [--base /path/] \
//!     [--shell <template.html>] [--no-build]
//! cargo run -p mq-bundle -- check <bundle-dir> <file.wasm>
//! ```
//!
//! Each repo has a `tools/mq-bundle` crate whose `main` is [`main`]: cargo
//! runs only its own workspace's binaries, and that keeps the bundler pinned
//! by the same macroquad-kit rev as the game, runnable in CI with only cargo.
//!
//! Every step exists because skipping it once cost a debugging round:
//!
//! 0. **Check the macroquad `[patch]`** before building. A gamekit client on
//!    a crates.io macroquad ≤ 0.4.16 fails, naming the lines to copy
//!    ([`MACROQUAD_PATCH`]); any other macroquad client gets a warning. A
//!    `[patch]` applies only in the workspace that builds, and without it one
//!    run in twelve grew a 256 MB glyph atlas.
//! 1. **Build** for wasm32 in release with `-C link-arg=--allow-undefined`
//!    appended to `RUSTFLAGS`. rustc 1.96 stopped passing it to wasm-ld, and
//!    miniquad and sapp-jsutils still declare bare `extern "C"` imports that
//!    the page's JS provides, so without it they do not link (and `cargo
//!    check` passes regardless).
//! 2. **Resolve** gl.js, sapp_jsutils.js and quad-net.js from the crate
//!    versions *this package* links ([`metadata`]), never a CDN or a copy
//!    committed beside the code: the plugin protocol is unversioned, and a
//!    mismatch is a blank canvas with no error.
//! 3. **Clear** the `.js`, `.wasm` and `index.html` a previous run left in
//!    `--dest` (nothing else), so a plugin no longer needed is not served
//!    beside a wasm that does not import it.
//! 4. **Stage** gl.js, sapp_jsutils.js when linked, quad-net.js when the wasm
//!    imports it (sockets *or* HTTP — checking one half once shipped socket
//!    clients without it), then every `*.js` in `--web`: the crate's own
//!    plugins, stamped by its `build.rs` from each plugin crate's `PLUGIN_JS`
//!    (or `macroquad_gamekit::web::stamp`). Two scripts registering the same
//!    miniquad plugin is an error: the page loads both, and the second silently
//!    wins. `--assets <dir>` replaces `<dest>/assets/` with the directory's
//!    contents, for a file the page fetches — a font for the text input, say.
//! 5. **Render** `index.html` from the shared shell ([`page::SHELL`]), with
//!    the wasm content-hashed into `load()`: an unversioned `load("x.wasm")`
//!    lets a proxy or the browser keep a stale build alive, invisibly.
//!    `--base /pump/` prefixes every path, for a page served at a bare path.
//! 6. **Check** that every function the wasm imports is provided by a staged
//!    script ([`imports`]). gl.js stubs a missing import rather than refusing
//!    to load, so a missing plugin was a silent game or a dead socket; here it
//!    is a build failure naming the functions.

pub mod imports;
pub mod metadata;
pub mod page;

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// What to bundle and where.
#[derive(Clone, Debug)]
pub struct Options {
    /// The cargo package to build (`-p`).
    pub package: String,
    /// The artifact's file name, e.g. `game.wasm` (a bin target's name).
    pub wasm: String,
    pub title: String,
    pub dest: PathBuf,
    /// The crate's plugin directory: every `*.js` in it is staged.
    pub web: Option<PathBuf>,
    /// Extra files the page fetches (a font): copied into `<dest>/assets/`,
    /// which is replaced wholesale each run.
    pub assets: Option<PathBuf>,
    /// Prefix for every script and the wasm in the page, e.g. `/pump/`.
    pub base: String,
    /// A page template other than the shared shell.
    pub shell: Option<PathBuf>,
    /// Build first (`false`: stage what is already built).
    pub build: bool,
    /// Where to run cargo: the workspace the package belongs to.
    pub cwd: PathBuf,
}

/// What a bundle run produced.
#[derive(Debug)]
pub struct Bundle {
    pub dest: PathBuf,
    /// Scripts in load order, as named in the page.
    pub scripts: Vec<String>,
    pub wasm_bytes: u64,
    pub cache_key: String,
    pub imports: usize,
}

fn io(what: &str, path: &Path, e: std::io::Error) -> String {
    format!("{what} {}: {e}", path.display())
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::copy(from, to)
        .map(|_| ())
        .map_err(|e| format!("copy {} → {}: {e}", from.display(), to.display()))
}

fn build(opts: &Options) -> Result<(), String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let flags = std::env::var("RUSTFLAGS").unwrap_or_default();
    let flags = format!("{flags} -C link-arg=--allow-undefined")
        .trim()
        .to_owned();
    eprintln!(
        "→ building {} for wasm32-unknown-unknown (release)",
        opts.package
    );
    let status = Command::new(cargo)
        .args([
            "build",
            "--target",
            "wasm32-unknown-unknown",
            "--release",
            "-p",
            &opts.package,
        ])
        .env("RUSTFLAGS", flags)
        .current_dir(&opts.cwd)
        .status()
        .map_err(|e| format!("run cargo build: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("cargo build -p {} failed", opts.package))
    }
}

/// The JS files in a directory, sorted by name: (file name, contents).
fn scripts_in(dir: &Path) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| io("read", dir, e))? {
        let path = entry.map_err(|e| io("read", dir, e))?.path();
        if path.extension().is_some_and(|e| e == "js") {
            let name = path
                .file_name()
                .expect("a file")
                .to_string_lossy()
                .into_owned();
            let text = std::fs::read_to_string(&path).map_err(|e| io("read", &path, e))?;
            out.push((name, text));
        }
    }
    out.sort();
    Ok(out)
}

/// The miniquad plugin names a page script registers.
fn plugin_names(js: &str) -> Vec<String> {
    const CALL: &str = "miniquad_add_plugin";
    let mut names = Vec::new();
    let mut rest = js;
    while let Some(at) = rest.find(CALL) {
        rest = &rest[at + CALL.len()..];
        // The object literal follows and is a few hundred bytes at most, so a
        // bounded search cannot run into the next call.
        let window = &rest[..rest.len().min(400)];
        if let Some(name) = field(window, "name") {
            names.push(name);
        }
    }
    names
}

/// The string value of `key: '…'` or `key: "…"` in `text`, if any.
fn field(text: &str, key: &str) -> Option<String> {
    let mut rest = text;
    loop {
        let at = rest.find(key)?;
        let after = rest[at + key.len()..].trim_start();
        let after = after.strip_prefix(':').map(str::trim_start);
        let Some(after) = after else {
            rest = &rest[at + key.len()..];
            continue;
        };
        let (quote, body) = match after.as_bytes().first() {
            Some(b'\'') => ('\'', &after[1..]),
            Some(b'"') => ('"', &after[1..]),
            _ => {
                rest = &rest[at + key.len()..];
                continue;
            }
        };
        let end = body.find(quote)?;
        return Some(body[..end].to_owned());
    }
}

/// Refuse two page scripts that register the same miniquad plugin: the page
/// loads both, and the second wins with no error. A renamed page script left
/// in the directory is the common cause.
fn check_plugins(scripts: &[(String, String)]) -> Result<(), String> {
    let mut seen: Vec<(String, String)> = Vec::new();
    for (file, text) in scripts {
        for name in plugin_names(text) {
            if let Some((_, first)) = seen.iter().find(|(n, _)| *n == name) {
                return Err(format!(
                    "{first} and {file} both register the miniquad plugin `{name}`; delete the \
                     superseded script — the page loads both, and the second wins silently"
                ));
            }
            seen.push((name, file.clone()));
        }
    }
    Ok(())
}

/// Copy `from`'s contents into `to`, replacing `to` first so a deleted asset
/// does not linger. Returns how many files were copied.
fn stage_assets(from: &Path, to: &Path) -> Result<usize, String> {
    if !from.is_dir() {
        return Err(format!("--assets {} is not a directory", from.display()));
    }
    if to.exists() {
        std::fs::remove_dir_all(to).map_err(|e| io("remove", to, e))?;
    }
    std::fs::create_dir_all(to).map_err(|e| io("create", to, e))?;
    let mut copied = 0;
    copy_tree(from, to, &mut copied)?;
    Ok(copied)
}

fn copy_tree(from: &Path, to: &Path, copied: &mut usize) -> Result<(), String> {
    for entry in std::fs::read_dir(from).map_err(|e| io("read", from, e))? {
        let path = entry.map_err(|e| io("read", from, e))?.path();
        let target = to.join(path.file_name().expect("a file"));
        if path.is_dir() {
            std::fs::create_dir_all(&target).map_err(|e| io("create", &target, e))?;
            copy_tree(&path, &target, copied)?;
        } else {
            copy(&path, &target)?;
            *copied += 1;
        }
    }
    Ok(())
}

fn report_unprovided(
    wasm: &str,
    dir: &Path,
    gap: &imports::Unprovided,
    scripts: &[String],
) -> String {
    let mut msg = format!(
        "{wasm} imports what no script in {} provides:\n",
        dir.display()
    );
    for f in &gap.foreign {
        msg += &format!("    {f}   (not miniquad's `env` module — wasm-bindgen?)\n");
    }
    for m in &gap.missing {
        msg += &format!("    env.{m}\n");
    }
    msg += &format!(
        "  scripts: {}\n",
        if scripts.is_empty() {
            "none".into()
        } else {
            scripts.join(", ")
        }
    );
    msg += "  A miniquad plugin is missing from the page; gl.js would stub these and the game\n";
    msg += "  would fail quietly. Stamp the plugin's JS into the crate's --web directory from\n";
    msg += "  build.rs (a crate's PLUGIN_JS, or macroquad_gamekit::web::stamp).";
    msg
}

/// Check an already-staged bundle: every function `wasm` imports is provided
/// by a `*.js` in `dir`. Returns how many imports were checked.
pub fn check(dir: &Path, wasm: &str) -> Result<usize, String> {
    let path = dir.join(wasm);
    let bytes = std::fs::read(&path).map_err(|e| io("read", &path, e))?;
    let imports =
        imports::function_imports(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let scripts = scripts_in(dir)?;
    let gap = imports::unprovided(&imports, scripts.iter().map(|(_, s)| s.as_str()));
    if gap.is_empty() {
        Ok(imports.len())
    } else {
        let names: Vec<String> = scripts.into_iter().map(|(n, _)| n).collect();
        Err(report_unprovided(wasm, dir, &gap, &names))
    }
}

/// The `[patch]` every consumer of the gamekit must carry, as it is written.
pub const MACROQUAD_PATCH: &str = r#"[patch.crates-io]
macroquad = { git = "https://github.com/not-fl3/macroquad", rev = "8d602d3f8ebb8a3a582ee15d92983bbb81ee3ef6" }"#;

/// Fail a gamekit client built on a crates.io macroquad that still has the
/// glyph-atlas bug, and warn any other client that is. A `[patch]` applies
/// only in the workspace that builds, so forgetting to copy it is silent:
/// the game works, and one run in twelve grows a 256 MB font atlas.
fn check_macroquad_patch(package: &str, linked: &metadata::Linked) -> Result<(), String> {
    let metadata::PatchCheck::Unpatched { version, gamekit } = linked.macroquad_patch() else {
        return Ok(());
    };
    let msg = format!(
        "{package} links macroquad {version} from crates.io, which doubles its glyph atlas at \
         random (256 MB on a bad run). Add this to the root Cargo.toml of the workspace that \
         builds it:\n\n{MACROQUAD_PATCH}\n"
    );
    if gamekit {
        Err(msg)
    } else {
        eprintln!("⚠ {msg}");
        Ok(())
    }
}

/// Build (unless told not to), stage and check one bundle.
pub fn bundle(opts: &Options) -> Result<Bundle, String> {
    let linked = metadata::linked(&opts.cwd, &opts.package)?;
    check_macroquad_patch(&opts.package, &linked)?;
    if opts.build {
        build(opts)?;
    }
    let wasm_path = linked
        .target_dir
        .join("wasm32-unknown-unknown/release")
        .join(&opts.wasm);
    let wasm_bytes = std::fs::read(&wasm_path).map_err(|e| {
        format!(
            "{} not built (is --wasm the bin target's name?): {e}",
            wasm_path.display()
        )
    })?;
    let imports = imports::function_imports(&wasm_bytes)
        .map_err(|e| format!("{}: {e}", wasm_path.display()))?;

    // 3. Clear what a previous run staged.
    let dest = &opts.dest;
    std::fs::create_dir_all(dest).map_err(|e| io("create", dest, e))?;
    for entry in std::fs::read_dir(dest).map_err(|e| io("read", dest, e))? {
        let path = entry.map_err(|e| io("read", dest, e))?.path();
        let ours = path.extension().is_some_and(|e| e == "js" || e == "wasm")
            || path.file_name().is_some_and(|n| n == "index.html");
        if ours && path.is_file() {
            std::fs::remove_file(&path).map_err(|e| io("remove", &path, e))?;
        }
    }

    // 4. Stage the scripts, in load order.
    let mut staged: Vec<(String, String)> = Vec::new();
    let mut stage_crate_js =
        |krate: &str, rel: &str, as_name: &str, required: bool| -> Result<(), String> {
            match linked.crate_dir(krate)? {
                Some(dir) => {
                    let from = dir.join(rel);
                    let text = std::fs::read_to_string(&from).map_err(|e| io("read", &from, e))?;
                    copy(&from, &dest.join(as_name))?;
                    staged.push((as_name.to_owned(), text));
                    Ok(())
                }
                None if required => Err(format!(
                    "{} does not link {krate}; is it a macroquad client?",
                    opts.package
                )),
                None => Ok(()),
            }
        };
    stage_crate_js("miniquad", "js/gl.js", "gl.js", true)?;
    stage_crate_js(
        "sapp-jsutils",
        "js/sapp_jsutils.js",
        "sapp_jsutils.js",
        false,
    )?;
    let wants_net = imports
        .iter()
        .any(|i| i.name.starts_with("ws_") || i.name.starts_with("http_"));
    if wants_net {
        stage_crate_js("quad-net", "js/quad-net.js", "quad-net.js", true)?;
    }
    if let Some(web) = &opts.web {
        let scripts = scripts_in(web)?;
        check_plugins(&scripts)?;
        for (name, text) in scripts {
            if staged.iter().any(|(n, _)| *n == name) {
                return Err(format!(
                    "{} has its own {name}, which the bundler stages itself; delete it",
                    web.display()
                ));
            }
            copy(&web.join(&name), &dest.join(&name))?;
            staged.push((name, text));
        }
    }
    copy(&wasm_path, &dest.join(&opts.wasm))?;
    if let Some(assets) = &opts.assets {
        let files = stage_assets(assets, &dest.join("assets"))?;
        eprintln!("→ staged {files} asset(s) into assets/");
    }

    // 5. The page.
    let hash = Sha256::digest(&wasm_bytes);
    let cache_key: String = hash.iter().take(6).map(|b| format!("{b:02x}")).collect();
    let template = match &opts.shell {
        Some(p) => std::fs::read_to_string(p).map_err(|e| io("read", p, e))?,
        None => page::SHELL.to_owned(),
    };
    let srcs: Vec<String> = staged
        .iter()
        .map(|(n, _)| format!("{}{n}", opts.base))
        .collect();
    let wasm_src = format!("{}{}?v={cache_key}", opts.base, opts.wasm);
    let html = page::render(&template, &opts.title, &srcs, &wasm_src, &opts.base)?;
    let index = dest.join("index.html");
    std::fs::write(&index, html).map_err(|e| io("write", &index, e))?;
    let manifest = dest.join(page::MANIFEST);
    std::fs::write(&manifest, page::manifest(&opts.title))
        .map_err(|e| io("write", &manifest, e))?;

    // 6. Every import provided.
    let gap = imports::unprovided(&imports, staged.iter().map(|(_, s)| s.as_str()));
    let names: Vec<String> = staged.iter().map(|(n, _)| n.clone()).collect();
    if !gap.is_empty() {
        return Err(report_unprovided(&opts.wasm, dest, &gap, &names));
    }
    Ok(Bundle {
        dest: dest.clone(),
        scripts: names,
        wasm_bytes: wasm_bytes.len() as u64,
        cache_key,
        imports: imports.len(),
    })
}

const USAGE: &str = "\
usage: mq-bundle --package <crate> --wasm <bin>.wasm --title <title> --dest <dir>
                 [--web <dir>] [--assets <dir>] [--base /path/] [--shell <template.html>]
                 [--no-build]
       mq-bundle check <bundle-dir> <file.wasm>";

enum Cli {
    Bundle(Options),
    Check(PathBuf, String),
}

fn parse(args: &[String], cwd: PathBuf) -> Result<Cli, String> {
    if args.first().is_some_and(|a| a == "check") {
        return match args {
            [_, dir, wasm] => Ok(Cli::Check(PathBuf::from(dir), wasm.clone())),
            _ => Err("check takes <bundle-dir> <file.wasm>".into()),
        };
    }
    let (mut package, mut wasm, mut title, mut dest) = (None, None, None, None);
    let (mut web, mut assets, mut shell, mut base, mut build) =
        (None, None, None, String::new(), true);
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let mut value = || {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag.as_str() {
            "--package" => package = Some(value()?),
            "--wasm" => wasm = Some(value()?),
            "--title" => title = Some(value()?),
            "--dest" => dest = Some(PathBuf::from(value()?)),
            "--web" => web = Some(PathBuf::from(value()?)),
            "--assets" => assets = Some(PathBuf::from(value()?)),
            "--shell" => shell = Some(PathBuf::from(value()?)),
            "--base" => base = value()?,
            "--no-build" => build = false,
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    fn need<T>(v: Option<T>, name: &str) -> Result<T, String> {
        v.ok_or_else(|| format!("--{name} is required"))
    }
    if !base.is_empty() && !base.ends_with('/') {
        base.push('/');
    }
    Ok(Cli::Bundle(Options {
        package: need(package, "package")?,
        wasm: need(wasm, "wasm")?,
        title: need(title, "title")?,
        dest: need(dest, "dest")?,
        web,
        assets,
        base,
        shell,
        build,
        cwd,
    }))
}

/// The `mq-bundle` command: a consuming repo's `tools/mq-bundle/src/main.rs`
/// is `fn main() -> std::process::ExitCode { macroquad_bundle::main() }`.
pub fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let result = parse(&args, cwd).and_then(|cli| match cli {
        Cli::Check(dir, wasm) => check(&dir, &wasm)
            .map(|n| println!("✓ all {n} imported functions provided by {}", dir.display())),
        Cli::Bundle(opts) => bundle(&opts).map(|b| {
            println!(
                "✓ {} → {} ({} KB wasm, cache key {}, {} imports provided by {})",
                opts.title,
                b.dest.display(),
                b.wasm_bytes / 1024,
                b.cache_key,
                b.imports,
                b.scripts.join(", ")
            )
        }),
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) if e.is_empty() => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("✗ {e}");
            if e.starts_with("--") || e.starts_with("unknown") || e.starts_with("check takes") {
                eprintln!("{USAGE}");
            }
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &[&str]) -> Vec<String> {
        s.iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn parses_a_bundle_with_a_base_that_gains_its_slash() {
        let cli = parse(
            &args(&[
                "--package",
                "p",
                "--wasm",
                "p.wasm",
                "--title",
                "T",
                "--dest",
                "d",
                "--base",
                "/pump",
                "--no-build",
            ]),
            PathBuf::from("/w"),
        );
        let Ok(Cli::Bundle(o)) = cli else {
            panic!("not a bundle")
        };
        assert_eq!(o.base, "/pump/");
        assert!(!o.build);
        assert_eq!(o.cwd, PathBuf::from("/w"));
    }

    #[test]
    fn a_missing_flag_is_named() {
        let Err(e) = parse(&args(&["--package", "p"]), PathBuf::new()) else {
            panic!("parsed")
        };
        assert_eq!(e, "--wasm is required");
    }

    #[test]
    fn check_finds_a_missing_plugin_in_a_staged_bundle() {
        let dir = std::env::temp_dir().join(format!("mq-bundle-check-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let wasm = imports::tests::module(&[("env", "glClear"), ("env", "audio_init")]);
        std::fs::write(dir.join("g.wasm"), wasm).expect("write wasm");
        std::fs::write(dir.join("gl.js"), "function glClear() {}").expect("write js");
        let err = check(&dir, "g.wasm").expect_err("audio_init is unprovided");
        assert!(err.contains("env.audio_init"), "{err}");
        std::fs::write(
            dir.join("audio.js"),
            "importObject.env.audio_init = audio_init;",
        )
        .expect("write js");
        assert_eq!(check(&dir, "g.wasm"), Ok(2));
        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn parses_a_bundle_with_assets() {
        let cli = parse(
            &args(&[
                "--package",
                "p",
                "--wasm",
                "p.wasm",
                "--title",
                "T",
                "--dest",
                "d",
                "--assets",
                "app/assets",
                "--no-build",
            ]),
            PathBuf::from("/w"),
        );
        let Ok(Cli::Bundle(o)) = cli else {
            panic!("not a bundle")
        };
        assert_eq!(o.assets, Some(PathBuf::from("app/assets")));
    }

    #[test]
    fn a_page_scripts_plugin_name_is_read_from_its_registration() {
        assert_eq!(
            plugin_names(
                "miniquad_add_plugin({ register_plugin, version: 1, name: 'app_shell' });"
            ),
            ["app_shell"]
        );
        // The minified form has no spaces, and a double-quoted name.
        assert_eq!(
            plugin_names("miniquad_add_plugin({name:\"gamepads\",version:\"0.1.7\"});"),
            ["gamepads"]
        );
        assert!(plugin_names("function register_plugin() {}").is_empty());
    }

    #[test]
    fn two_scripts_registering_one_plugin_are_refused() {
        let scripts = vec![
            (
                "app.js".to_string(),
                "miniquad_add_plugin({ register_plugin, version: 1, name: 'app_shell' });"
                    .to_string(),
            ),
            (
                "gamekit-app-shell.js".to_string(),
                "miniquad_add_plugin({ register_plugin, version: 2, name: 'app_shell' });"
                    .to_string(),
            ),
        ];
        let err = check_plugins(&scripts).expect_err("the same plugin twice");
        assert!(err.contains("app.js"), "{err}");
        assert!(err.contains("gamekit-app-shell.js"), "{err}");
        assert!(err.contains("app_shell"), "{err}");

        let distinct = vec![
            ("a.js".to_string(), scripts[0].1.clone()),
            (
                "b.js".to_string(),
                "miniquad_add_plugin({ register_plugin, version: 1, name: 'other' });".to_string(),
            ),
        ];
        assert_eq!(check_plugins(&distinct), Ok(()));
    }

    #[test]
    fn assets_are_staged_and_the_directory_is_replaced() {
        let base = std::env::temp_dir().join(format!("mq-bundle-assets-{}", std::process::id()));
        let from = base.join("from");
        let to = base.join("dest/assets");
        std::fs::create_dir_all(from.join("nested")).expect("mkdir");
        std::fs::write(from.join("face.ttf"), b"font").expect("write");
        std::fs::write(from.join("nested/theme.css"), b"body{}").expect("write");

        assert_eq!(stage_assets(&from, &to), Ok(2));
        assert!(to.join("face.ttf").exists());
        assert!(to.join("nested/theme.css").exists());

        // A file deleted from the source does not linger in the bundle.
        std::fs::remove_file(from.join("face.ttf")).expect("remove");
        assert_eq!(stage_assets(&from, &to), Ok(1));
        assert!(!to.join("face.ttf").exists());
        assert!(to.join("nested/theme.css").exists());

        assert!(stage_assets(&base.join("missing"), &to).is_err());
        std::fs::remove_dir_all(&base).expect("clean up");
    }
}
