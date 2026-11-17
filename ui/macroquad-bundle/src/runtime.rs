//! The page's scripts as one content-hashed file.
//!
//! A bundle's page scripts — gl.js, sapp_jsutils.js, quad-net.js, text.js and
//! the crate's own plugins — used to be one `<script src>` each, under fixed
//! names. Only the wasm carried a cache key, so a browser or a proxy that kept
//! an old `text.js` paired it with a new wasm, and miniquad's version
//! handshake then refused the plugin: a page that did not boot, after a deploy
//! that changed nothing it could see. Nine requests stood in front of the first
//! frame, too.
//!
//! Now they go as one file named by its own content, `runtime.<hash>.js`: a
//! new build is a new name, so it can never be paired with another build's
//! wasm, and the old name can be cached for ever. Only `index.html`, which
//! names both, must be fetched fresh.
//!
//! **Each script still runs as its own script.** The file is a list of the
//! sources and a loop that inserts each as an inline `<script>`, in order;
//! inserted inline scripts run synchronously, one at a time, exactly as the
//! separate tags did. Simply joining the sources would not do that: gl.js opens
//! with `"use strict"`, which would make every plugin after it strict, and a
//! plugin's own directive mid-file would stop applying; an exception at one
//! script's top level would stop the rest from loading. The plugin protocol
//! also needs the scripts to share the global scope (`load`,
//! `miniquad_add_plugin` and `js_object` are globals one defines and the
//! others call), so they cannot each be wrapped in a function. Each keeps a
//! `sourceURL`, so devtools and stack traces still name `gl.js`, `text.js`.

use sha2::{Digest, Sha256};

/// The runtime file: its name and its text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Runtime {
    /// `runtime.<hash>.js`, the hash of [`Runtime::text`].
    pub file: String,
    pub text: String,
}

/// The runtime for `scripts` — (file name, source) in load order.
pub fn runtime(scripts: &[(String, String)]) -> Runtime {
    let mut text = format!(
        "// The page's scripts, each run as its own <script> in this order: see \
         macroquad-bundle's runtime.rs.\n(function () {{\n  var parts = {OPEN}"
    );
    let rows: Vec<String> = scripts
        .iter()
        .map(|(name, source)| {
            let name = serde_json::to_string(name).expect("a string serialises");
            let source = serde_json::to_string(source).expect("a string serialises");
            format!("    [{name}, {source}]")
        })
        .collect();
    // Plain JSON between the markers, so [`sources`] can read a staged bundle back.
    text.push_str(&rows.join(",\n"));
    text.push_str(CLOSE);
    text.push_str(
        "\n  var into = document.head || document.documentElement;\n  for (var i = 0; i < \
         parts.length; i++) {\n    var s = document.createElement(\"script\");\n    s.text = \
         parts[i][1] + \"\\n//# sourceURL=\" + parts[i][0];\n    into.appendChild(s);\n  }\n})();\n",
    );
    let hash = Sha256::digest(text.as_bytes());
    let key: String = hash.iter().take(6).map(|b| format!("{b:02x}")).collect();
    Runtime {
        file: format!("runtime.{key}.js"),
        text,
    }
}

const OPEN: &str = "[\n";
const CLOSE: &str = "\n  ];";

/// The scripts inside a runtime's text, as [`runtime`] was given them: what
/// `mq-bundle check` reads a staged bundle's sources from. `None` for text this
/// module did not write.
pub fn sources(text: &str) -> Option<Vec<(String, String)>> {
    let start = text.find("var parts = ")? + "var parts = ".len();
    let end = text[start..].find(CLOSE)? + start + CLOSE.len() - 1;
    serde_json::from_str(&text[start..end]).ok()
}

/// Whether `name` is a runtime file this bundler wrote.
pub fn is_runtime(name: &str) -> bool {
    name.strip_prefix("runtime.")
        .and_then(|rest| rest.strip_suffix(".js"))
        .is_some_and(|key| key.len() == 12 && key.chars().all(|c| c.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts() -> Vec<(String, String)> {
        vec![
            ("gl.js".into(), "\"use strict\";\nvar a = 1;".into()),
            (
                "quad-net.js".into(),
                "register_plugin = function () {}".into(),
            ),
            ("text.js".into(), "(function () { '</script>' })();".into()),
        ]
    }

    #[test]
    fn the_scripts_go_in_their_order_each_named() {
        let r = runtime(&parts());
        let gl = r.text.find("[\"gl.js\"").expect("gl.js");
        let net = r.text.find("[\"quad-net.js\"").expect("quad-net.js");
        let text = r.text.find("[\"text.js\"").expect("text.js");
        assert!(gl < net && net < text, "load order kept");
        assert!(r.text.contains("//# sourceURL="));
    }

    /// Each source is a string, run as its own script: gl.js's `"use strict"` is gl.js's alone.
    #[test]
    fn a_scripts_directive_stays_its_own() {
        let r = runtime(&parts());
        assert!(
            r.text
                .contains(r#"["gl.js", "\"use strict\";\nvar a = 1;"],"#),
            "{}",
            r.text
        );
        // Nothing of any source is code at the file's top level: the only statement is the loader.
        assert!(r.text.starts_with("// The page's scripts"));
        assert!(!r.text.contains("\n\"use strict\""));
    }

    #[test]
    fn the_name_follows_the_content() {
        let a = runtime(&parts());
        let mut changed = parts();
        changed[2].1.push(' ');
        let b = runtime(&changed);
        assert_ne!(a.file, b.file);
        assert_eq!(
            a.file,
            runtime(&parts()).file,
            "the same scripts, the same name"
        );
        assert!(is_runtime(&a.file), "{}", a.file);
    }

    #[test]
    fn a_runtime_reads_back_as_its_scripts() {
        let r = runtime(&parts());
        assert_eq!(sources(&r.text), Some(parts()));
        assert_eq!(sources(&runtime(&[]).text), Some(Vec::new()));
        assert_eq!(sources("console.log(1)"), None);
    }

    #[test]
    fn only_a_runtime_name_is_one() {
        assert!(is_runtime("runtime.0123456789ab.js"));
        assert!(!is_runtime("runtime.js"));
        assert!(!is_runtime("runtime.0123456789ab.wasm"));
        assert!(!is_runtime("runtime.xyz.js"));
        assert!(!is_runtime("gl.js"));
    }

    /// The runtime runs: each part becomes a script element with its source and name. Checked
    /// against the generated text rather than a JS engine, which the bundler does not carry.
    #[test]
    fn the_loader_inserts_each_part_as_a_script() {
        let r = runtime(&parts());
        for needle in [
            "document.createElement(\"script\")",
            "s.text = parts[i][1]",
            "into.appendChild(s)",
        ] {
            assert!(r.text.contains(needle), "loader lost {needle:?}");
        }
    }
}
