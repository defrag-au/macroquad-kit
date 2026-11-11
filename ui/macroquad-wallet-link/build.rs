//! Stamp the app's plugin JS into `web/`, so the page loads the plugins the
//! wasm was linked against: `wallet.js`, `evm.js` and `midnight.js`
//! (wallet-miniquad), `platform.js` (miniquad-platform), the app shell's page
//! half as `gamekit-app-shell.js` (macroquad-gamekit's `PageScript::AppShell`)
//! and `discord-auth.js` (the session a Discord login hands back).
//! `mq-bundle --web ui/macroquad-wallet-link/web` stages every `*.js` there.
//! Generated, so `web/` is gitignored.

use std::fs;
use std::path::Path;

use macroquad_gamekit::web::{PageScript, stamp_scripts};

fn main() {
    let web = Path::new(env!("CARGO_MANIFEST_DIR")).join("web");
    fs::create_dir_all(&web).expect("create web/");
    fs::write(web.join("wallet.js"), wallet_miniquad::PLUGIN_JS).expect("write wallet.js");
    fs::write(web.join("evm.js"), wallet_miniquad::EVM_PLUGIN_JS).expect("write evm.js");
    fs::write(web.join("midnight.js"), wallet_miniquad::MIDNIGHT_PLUGIN_JS)
        .expect("write midnight.js");
    fs::write(web.join("platform.js"), miniquad_platform::PLUGIN_JS).expect("write platform.js");
    stamp_scripts(&web, &[PageScript::AppShell]).expect("stamp the app shell page script");
    // Before the app shell moved into the gamekit this script was stamped as
    // `app.js`. `stamp_scripts` writes and never removes, and `mq-bundle`
    // stages every `*.js` in `--web`, so a `web/` built before the move would
    // carry both copies and the page would load the page script twice.
    match fs::remove_file(web.join("app.js")) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => panic!("remove the superseded app.js: {e}"),
    }
    fs::write(
        web.join("discord-auth.js"),
        discord_auth::miniquad::PLUGIN_JS,
    )
    .expect("write discord-auth.js");
    println!("cargo:rerun-if-changed=build.rs");
    // The JS these consts carry lives in the crates' own directories, and cargo
    // re-runs a build script only for the files it is told to watch. Without
    // these, editing a plugin's JS leaves `web/` holding the previous copy, and
    // the bundle ships a page whose bridge is a version behind its wasm — the
    // exact silent mismatch `mq-bundle`'s import check cannot see, because the
    // imports are still provided, just by older code.
    for js in [
        "../wallet-miniquad/js/wallet.js",
        "../wallet-miniquad/js/evm.js",
        "../wallet-miniquad/js/midnight.js",
        "../miniquad-platform/js/platform.js",
        "../../gaming/macroquad-gamekit/js/app-shell.js",
        "../../discord-auth/js/discord-auth.js",
    ] {
        println!("cargo:rerun-if-changed={js}");
    }
}
