//! Stamp the web build's plugin JS into `web/`.
//!
//! `web/wallet.js`, `web/evm.js`, `web/platform.js` and
//! `web/gamekit-app-shell.js` are written from each plugin crate's `PLUGIN_JS`
//! (the app shell's through the gamekit's page scripts), so the plugin a
//! browser loads is always the one the wasm was linked against. The bundler
//! (`mq-bundle --web ui/macroquad-storybook/web`) stages every `*.js` there.
//! Generated, so `web/` is gitignored.

use std::fs;
use std::path::Path;

use macroquad_gamekit::web::{PageScript, stamp_scripts};

fn main() {
    let web = Path::new(env!("CARGO_MANIFEST_DIR")).join("web");
    fs::create_dir_all(&web).expect("create web/");
    fs::write(web.join("wallet.js"), wallet_miniquad::PLUGIN_JS).expect("write wallet.js");
    fs::write(web.join("evm.js"), wallet_miniquad::EVM_PLUGIN_JS).expect("write evm.js");
    fs::write(web.join("platform.js"), miniquad_platform::PLUGIN_JS).expect("write platform.js");
    stamp_scripts(&web, &[PageScript::AppShell]).expect("stamp the app shell page script");
    println!("cargo:rerun-if-changed=build.rs");
}
