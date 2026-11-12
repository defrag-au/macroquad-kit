//! Guard: a rounded shape or a circle is drawn by THIS crate, never by
//! macroquad's own primitive.
//!
//! ## Why a test and not just a convention
//!
//! `draw_circle`, `draw_circle_lines` and `draw_arc` are in
//! `macroquad::prelude::*`, which every widget module already imports with a
//! glob. That means the tessellated versions need no import at all — the path of
//! least resistance is the wrong one, and a paragraph in `AGENTS.md` does not
//! beat a name that is already in scope.
//!
//! Rust resolves this cleanly: an explicit `use` shadows a glob. So each module
//! that draws a circle imports the kit's version, the call sites are untouched,
//! and the prelude's becomes unreachable there. The import looks like a no-op to
//! a reader, which is exactly why it needs a test — a new module that forgets it
//! compiles, runs, and quietly draws a twenty-sided circle.
//!
//! `tests/text_sizes.rs` is the precedent: a rule the type system cannot carry,
//! enforced as a source scan so the default path is the right one.
//!
//! ## The rule
//!
//! ```ignore
//! // NO  — the prelude's, which is a 20-gon and a 30-gon
//! draw_circle(x, y, r, color);
//! // YES — the same call, with the kit's import above it
//! use macroquad_widgets::shape::draw_circle;
//! ```
//!
//! The definitions themselves (`shape.rs`) and the renderer behind them
//! (`sdf.rs`) are the two files exempt, because they are where the fallback
//! calls macroquad on purpose.

use std::path::{Path, PathBuf};

/// Everything under `ui/*/src` and `gaming/*/src` in this workspace: the
/// widget library, the app shell, the gamekit and the screens built on them.
fn sources() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("ui/macroquad-widgets sits two levels under the workspace root")
        .to_path_buf();
    let mut files = Vec::new();
    for group in ["ui", "gaming"] {
        let Ok(entries) = std::fs::read_dir(root.join(group)) else {
            continue;
        };
        for entry in entries.flatten() {
            collect(&entry.path().join("src"), &mut files);
        }
    }
    files.sort();
    files
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Every `use` statement, joined into one string, so a braced list spread over
/// several lines still reads as names.
fn imports(src: &str) -> String {
    let mut out = String::new();
    let mut open = false;
    for line in src.lines() {
        let l = line.trim();
        if !open && l.starts_with("use ") {
            open = true;
        }
        if open {
            out.push_str(l);
            out.push('\n');
            if l.ends_with(';') {
                open = false;
            }
        }
    }
    out
}

/// The statement without its trailing comment, so a `// draw_circle(…)` in
/// prose is not a call.
fn code(line: &str) -> &str {
    match line.find("//") {
        Some(at) => &line[..at],
        None => line,
    }
}

const NAMES: [&str; 3] = ["draw_circle_lines", "draw_circle", "draw_arc"];

#[test]
fn circles_and_arcs_come_from_the_kit() {
    let files = sources();
    assert!(
        files.len() > 40,
        "the scan found {} files — the workspace root is probably wrong",
        files.len()
    );

    let mut offenders = Vec::new();
    let mut calls = 0usize;
    for path in &files {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name == "shape.rs" || name == "sdf.rs" {
            continue;
        }
        let Ok(src) = std::fs::read_to_string(path) else {
            continue;
        };
        let uses = imports(&src);
        for (n, line) in src.lines().enumerate() {
            let code = code(line);
            for imported in NAMES {
                let call = format!("{imported}(");
                if !code.contains(&call) {
                    continue;
                }
                calls += 1;
                if !uses.contains(imported) {
                    offenders.push(format!(
                        "{}:{} — `{imported}` is macroquad's here; import the kit's\n    {}",
                        path.display(),
                        n + 1,
                        line.trim()
                    ));
                }
            }
        }
    }

    assert!(
        calls > 50,
        "only {calls} circle/arc calls found — the scan is not reaching them"
    );
    assert!(
        offenders.is_empty(),
        "{} call(s) would draw macroquad's polygon rather than the kit's exact \
         shape.\nAdd `use macroquad_widgets::shape::{{draw_circle, …}};` (or \
         `use crate::shape::…` inside the widgets) to the module — the call \
         itself does not change.\n\n{}",
        offenders.len(),
        offenders.join("\n")
    );
}
