//! Guard: the macroquad surfaces carry no wasm-bindgen.
//!
//! wasm-bindgen can NEVER link under miniquad — the `gl.js` runtime carries no
//! wasm-bindgen glue, and that is permanent rather than a version problem. A
//! single wasm-bindgen crate anywhere in a macroquad client's tree is the
//! difference between the client building and not.
//!
//! This is the kit's half of the guard that used to live in
//! `shared-crates/cardano-tx/tests/miniquad_linkable.rs`: `cardano-tx` keeps
//! its own no-wasm-bindgen invariant there, and this test keeps the macroquad
//! crates clear of it after the extraction.
//!
//! # Why a `cargo tree` scrape and not a build
//!
//! Building for `wasm32-unknown-unknown` would compile wasm-bindgen happily;
//! the failure is at the *miniquad* link step, which is in a host this
//! workspace does not contain. Resolution is what decides the question here:
//! wasm-bindgen either is in the graph or it is not.
//!
//! ⚠️ A NATIVE check is blind to this. The wasm-bindgen crates are behind
//! `cfg(target_arch = "wasm32")`, so `cargo tree` without `--target` reports a
//! clean graph for a crate that is anything but.

use std::process::Command;

/// The feature set a macroquad host actually takes, and what it must not drag
/// in. One row per invariant, so a failure names the configuration rather than
/// leaving someone to work out which one broke.
struct Invariant {
    package: &'static str,
    /// Extra flags after `cargo tree -p <package>`.
    features: &'static [&'static str],
    why: &'static str,
}

const INVARIANTS: &[Invariant] = &[
    Invariant {
        package: "macroquad-widgets",
        features: &["--all-features"],
        why: "these widgets render INSIDE miniquad, so every feature they ship \
              (including `chain`) has to stay clear of wasm-bindgen",
    },
    Invariant {
        package: "macroquad-gamekit",
        features: &["--all-features"],
        why: "the game loop, audio and gamepads features run inside miniquad; \
              none may reach wasm-bindgen",
    },
    Invariant {
        package: "macroquad-app",
        features: &[],
        why: "the app shell's HTTP and plugin bridge must stay usable under \
              miniquad, where web-sys cannot",
    },
    Invariant {
        package: "macroquad-cnft",
        features: &[],
        why: "holdings and art load inside a game, not through a browser API \
              bridge",
    },
    Invariant {
        package: "macroquad-wallet-flow",
        features: &[],
        why: "the CIP-30 and EIP-1193 flows run under miniquad's wallet plugin",
    },
    Invariant {
        package: "arcade-harness",
        features: &["--all-features"],
        why: "both the Discord and browser-tab backends must stay linkable \
              under miniquad",
    },
];

/// `cargo tree` for one invariant, as lines.
fn tree(inv: &Invariant) -> Result<String, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut cmd = Command::new(cargo);
    cmd.args(["tree", "-p", inv.package])
        .args(inv.features)
        // `normal` only: a dev-dependency on wasm-bindgen would not be linked
        // into a host's binary, and flagging one would push someone to weaken a
        // test rather than fix a real edge.
        .args(["-e", "normal"])
        .args(["--target", "wasm32-unknown-unknown"])
        .current_dir(env!("CARGO_MANIFEST_DIR"));

    let out = cmd
        .output()
        .map_err(|e| format!("could not run `cargo tree`: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let package = inv.package;
        return Err(format!("`cargo tree -p {package}` failed:\n{stderr}"));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[test]
fn the_macroquad_surfaces_carry_no_wasm_bindgen() {
    for inv in INVARIANTS {
        let tree = match tree(inv) {
            Ok(t) => t,
            Err(e) => panic!("{e}"),
        };

        let offenders: Vec<&str> = tree
            .lines()
            .filter(|l| l.contains("wasm-bindgen"))
            .collect();

        assert!(
            offenders.is_empty(),
            "`{}` {} reaches wasm-bindgen, which can never link under \
             miniquad:\n{}\n\nWhy this matters: {}.\n\nFind the edge with:\n  \
             cargo tree -p {} {} -e normal --target wasm32-unknown-unknown \
             -i wasm-bindgen",
            inv.package,
            inv.features.join(" "),
            offenders.join("\n"),
            inv.why,
            inv.package,
            inv.features.join(" "),
        );
    }
}
