//! `jobs-board` — watch the directories `rig-eval task` writes, and start one.
//!
//! ```sh
//! cargo run -p jobs-board -- --root ../agent-playbook/.tmp/container-flows
//! cargo run -p jobs-board -- --root ../agent-playbook/.tmp/container-flows \
//!     --tasks ../agent-playbook/integrations/rig-eval/tasks \
//!     --runner ../agent-playbook/integrations/rig-eval/target/debug/rig-eval
//! ```
//!
//! The root may be one job directory or a directory of them. The board re-reads
//! every two seconds, so a running job's phase and verify pips move on their
//! own; a card opens the sheet with the run's facts. Its footer is what a viewer
//! can honestly do: `Reveal` opens the job directory (the report, patch and logs
//! are in it), and `Refresh` re-reads now.
//!
//! With `--tasks` and `--runner` it also offers the one thing a viewer with the
//! runner installed can honestly do: start a task. The picker lists the task
//! files, and a run writes `<root>/<task>-<unix seconds>/` — the board picks it
//! up on its next read. The runner's own stdout and stderr go to that
//! directory's `runner.log`; the child is never waited on in the frame loop and
//! is left running if the board closes, because a run is not the watcher's.
//!
//! Flags beyond [`harness::Args`]'s window and capture ones:
//!
//! - `--root <dir>`   the job directory, or the directory of them (required)
//! - `--now <unix>`   freeze the clock elapsed is measured against
//! - `--open <task>`  start with a job's sheet open (a substring of its task)
//! - `--tasks <dir>`  task JSONs to offer in the picker
//! - `--runner <bin>` the `rig-eval` to start them with (both are needed)
//!
//! `--shot` captures headlessly, as every macroquad tool does, and writes the
//! PNG and its text report beside it.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use jobs::{Job, Scan};
use jobs_board::{Action, ActionSet, Board, BoardEvent, BoardFrame, Launch};
use macroquad::prelude::*;
use macroquad_gamekit::harness;
use macroquad_widgets::cursor;
use macroquad_widgets::{Gestures, Painter, Theme};

/// How often the job directories are re-read.
const REFRESH: Duration = Duration::from_secs(2);
/// Frames drawn before a capture: hit rects and hover read one frame stale,
/// and the first frame has no layout from a previous pass.
const SETTLE_FRAMES: u32 = 3;

fn window_conf() -> Conf {
    let argv: Vec<String> = std::env::args().collect();
    let args = harness::Args::read();
    let quiet = argv.iter().any(|arg| arg == "--help") || !argv.iter().any(|arg| arg == "--root");
    let mut conf = args.window_conf(Conf {
        window_title: "container jobs".to_owned(),
        window_width: 900,
        window_height: 640,
        window_resizable: true,
        ..Default::default()
    });
    // No root, or a usage request: no window to flash before the message.
    if quiet {
        conf.headless = true;
    }
    conf
}

fn usage() {
    println!(
        "jobs-board — watch the container-flow job directories, and start one\n\n\
         usage: jobs-board --root <dir> [--tasks <dir> --runner <bin>]\n\
                [--size 900x640] [--shot out.png] [--fail-on-problems]\n\
        \n\
         <dir> is one job directory or a directory of them, as `rig-eval task --out` writes.\n\
         --open <task>   start with a job's sheet open (a substring of its task)\n\
         --now <unix>    freeze the clock elapsed is measured against\n\
         --tasks <dir>   task JSONs to offer; new runs go in --root as <task>-<unix>\n\
         --runner <bin>  the rig-eval binary that starts them (needs --tasks)\n\
         --shot <png>    capture the board headlessly and write the text report beside it"
    );
}

#[macroquad::main(window_conf)]
async fn main() {
    let args = harness::Args::read();
    if std::env::args().any(|arg| arg == "--help") {
        usage();
        // A macroquad context is not happy to be dropped on the way out of
        // `main`; every other early exit here exits the process too.
        std::process::exit(0);
    }
    let Some(root) = harness::flag("--root").map(PathBuf::from) else {
        usage();
        std::process::exit(2);
    };
    let opened = harness::flag("--open");
    let fixed_now = harness::flag("--now").and_then(|now| now.parse::<i64>().ok());

    // The picker is offered only when a host names both halves: what it may
    // start, and what starts it. A button that can do nothing is worse than no
    // button.
    let tasks = match harness::flag("--tasks") {
        Some(dir) => {
            let dir = PathBuf::from(dir);
            if !dir.is_dir() {
                eprintln!("--tasks {}: not a directory", dir.display());
                std::process::exit(2);
            }
            match read_tasks(&dir) {
                Ok(tasks) if tasks.launches.is_empty() => {
                    eprintln!("--tasks {}: no task JSON files", dir.display());
                    std::process::exit(2);
                }
                Ok(tasks) => Some(tasks),
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(2);
                }
            }
        }
        None => None,
    };
    let runner = match (tasks.is_some(), harness::flag("--runner")) {
        (true, Some(runner)) => {
            let runner = PathBuf::from(runner);
            match runner.canonicalize() {
                Ok(path) if path.is_file() => Some(path),
                Ok(_) => {
                    eprintln!("--runner {}: not a file", runner.display());
                    std::process::exit(2);
                }
                Err(error) => {
                    eprintln!("--runner {}: {error}", runner.display());
                    std::process::exit(2);
                }
            }
        }
        (false, Some(_)) => {
            eprintln!("--runner needs --tasks");
            std::process::exit(2);
        }
        (true, None) => {
            eprintln!("--tasks needs --runner");
            std::process::exit(2);
        }
        (false, None) => None,
    };
    if tasks.is_some() && is_job_dir(&root) {
        eprintln!("--tasks needs --root to be the directory of jobs, not one job");
        std::process::exit(2);
    }
    let launches: &[Launch] = tasks
        .as_ref()
        .map(|tasks| tasks.launches.as_slice())
        .unwrap_or(&[]);
    let mut children: Vec<(String, std::process::Child)> = Vec::new();

    // The same two faces the storybook and every other macroquad tool in this
    // workspace embed, from `ui/macroquad-storybook/assets/` where the kit keeps
    // them; the widget crate leaves loading to the host on purpose.
    let font = load_ttf_font_from_bytes(include_bytes!(
        "../../macroquad-storybook/assets/NotoSans-Bold.ttf"
    ))
    .ok();
    let mono = load_ttf_font_from_bytes(include_bytes!(
        "../../macroquad-storybook/assets/JetBrainsMono-Regular.ttf"
    ))
    .ok();
    let theme = Theme::PRESETS[0]();

    let mut scan = jobs::scan(&root);
    let mut last_problems = problems_text(&scan.problems);
    if !last_problems.is_empty() {
        eprintln!("{last_problems}");
    }
    let mut status = watch_status(&root, &scan);
    let mut board = match &opened {
        Some(want) => match scan
            .jobs
            .iter()
            .find(|job| job.task.contains(want.as_str()))
        {
            Some(job) => Board::opened_on_task(&scan.jobs, &job.task),
            None => {
                eprintln!("no job matching {want:?}; the board starts closed");
                Board::new()
            }
        },
        None => Board::new(),
    };
    let mut gestures = Gestures::new();
    let mut last_scan = Instant::now();
    let mut frames = 0u32;
    if args.shot.is_some() {
        harness::report::enable();
    }

    loop {
        let gesture = gestures.update();
        let dt = get_frame_time();

        // A capture is one moment; a live board follows the directories. Hand
        // the reader a refresh only when the window is not capturing, so a
        // review capture is of the state the run was given.
        if args.shot.is_none() && last_scan.elapsed() >= REFRESH {
            scan = jobs::scan(&root);
            last_scan = Instant::now();
            status = watch_status(&root, &scan);
            let text = problems_text(&scan.problems);
            if text != last_problems {
                if text.is_empty() {
                    println!("all job directories read");
                } else {
                    eprintln!("{text}");
                }
                last_problems = text;
            }
        }

        let now_s = fixed_now.unwrap_or_else(now_unix);
        let vp = args.viewport_points();
        let area = Rect::new(0.0, 0.0, vp.size.x, vp.size.y);
        let due = args.shot.is_some() && frames >= SETTLE_FRAMES;
        // The camera has to be the capture's before anything is cleared or
        // drawn, or the frame lands on the (headless) window instead.
        let capture = due.then(|| harness::Capture::window(&vp, Vec2::ZERO));
        let p = Painter::new(font.as_ref(), mono.as_ref(), theme, gesture.tap)
            .at_time(if args.capturing() { 0.0 } else { get_time() });
        clear_background(theme.color.bg_primary);
        let frame = BoardFrame {
            area,
            gesture: &gesture,
            dt,
            now_s,
            jobs: &scan.jobs,
            status: Some(&status),
            actions: live_actions,
            launches,
        };
        let event = board.draw(&p, frame);

        if let Some(capture) = capture {
            let path = args.shot.as_deref().expect("a capture has a path");
            let report = capture
                .save(path, &format!("jobs-board · {} jobs", scan.jobs.len()))
                .expect("write the capture and its report");
            println!("wrote {path}");
            if args.fail_on_problems {
                let (screen, bakes) = harness::report::surfaces();
                let mut problems = harness::report::problems(&screen);
                for face in &bakes {
                    problems.extend(harness::report::problems(face));
                }
                problems.retain(|problem| !args.allow.contains(&problem.kind()));
                if !problems.is_empty() {
                    eprintln!(
                        "{} text problem(s) in the capture; see {}",
                        problems.len(),
                        report.display()
                    );
                    std::process::exit(2);
                }
            }
            std::process::exit(0);
        }

        if let Some(event) = event {
            match event {
                BoardEvent::Opened { task } => status = format!("opened {task}"),
                BoardEvent::Action { task, index } => match index {
                    // `live_actions` order: Reveal, then Refresh.
                    0 => {
                        if let Some(job) = scan.jobs.iter().find(|job| job.task == task) {
                            status = match reveal(&job.dir) {
                                Ok(()) => format!("revealed {}", job.dir.display()),
                                Err(error) => {
                                    format!("could not reveal {}: {error}", job.dir.display())
                                }
                            };
                        }
                    }
                    1 => {
                        scan = jobs::scan(&root);
                        last_scan = Instant::now();
                        status = watch_status(&root, &scan);
                        let text = problems_text(&scan.problems);
                        if text != last_problems {
                            if !text.is_empty() {
                                eprintln!("{text}");
                            }
                            last_problems = text;
                        }
                    }
                    _ => {}
                },
                BoardEvent::Launch { index } => {
                    if let (Some(tasks), Some(runner)) = (&tasks, &runner)
                        && let Some(path) = tasks.paths.get(index)
                    {
                        status = match start(runner, path, &root, now_unix()) {
                            Ok((label, child)) => {
                                let line = format!("started {label}");
                                children.push((label, child));
                                line
                            }
                            Err(error) => error,
                        };
                        scan = jobs::scan(&root);
                        last_scan = Instant::now();
                    }
                }
            }
        }

        // A run is the runner's, not the watcher's: reap a child that has
        // finished, without ever waiting on one in the frame loop.
        let mut finished = None;
        children.retain_mut(|(label, child)| match child.try_wait() {
            Ok(Some(exit)) => {
                finished = Some(format!("{label} {}", exit_label(exit)));
                false
            }
            Ok(None) => true,
            Err(error) => {
                finished = Some(format!("{label}: {error}"));
                false
            }
        });
        if let Some(line) = finished {
            status = line;
        }

        cursor::finish_frame();
        next_frame().await;
        frames += 1;
    }
}

/// What the picker offers, and the files behind each row. Two parallel vectors
/// keep the renderer free of paths: the board reports an index, the host
/// resolves it.
struct Tasks {
    launches: Vec<Launch>,
    paths: Vec<PathBuf>,
}

/// Every `*.json` in `dir`, labelled by its file stem. Reading the task itself
/// is the runner's job: the picker only has to name what it offers, and a task
/// that turns out to be broken leaves a failed job rather than being hidden.
fn read_tasks(dir: &Path) -> Result<Tasks, String> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|error| format!("reading {}: {error}", dir.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    paths.sort();
    // The cards drop the repository prefix; the picker drops whatever prefix
    // every task shares, so the rows show what differs.
    let names: Vec<String> = paths.iter().map(|path| label(path)).collect();
    let prefix = shared_prefix(&names);
    let launches = names
        .into_iter()
        .map(|name| {
            let short = name
                .strip_prefix(&prefix)
                .unwrap_or(name.as_str())
                .to_owned();
            Launch { name: short }
        })
        .collect();
    Ok(Tasks { launches, paths })
}

/// The `repo-` prefix every task name shares: the longest common prefix,
/// truncated at its last `-`, so a name is never cut mid-word. Nothing shared
/// means nothing stripped.
fn shared_prefix(names: &[String]) -> String {
    let Some((first, rest)) = names.split_first() else {
        return String::new();
    };
    // One task has no sibling to differ from: its whole name is what the row shows.
    if rest.is_empty() {
        return String::new();
    }
    let mut end = first.len();
    for name in rest {
        end = end.min(common_len(&first[..end], name));
    }
    match first[..end].rfind('-') {
        Some(at) => first[..=at].to_owned(),
        None => String::new(),
    }
}

fn common_len(a: &str, b: &str) -> usize {
    let mut end = 0;
    for (left, right) in a.chars().zip(b.chars()) {
        if left != right {
            break;
        }
        end += left.len_utf8();
    }
    end
}

fn label(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// A directory the reader treats as one job rather than a root of them.
fn is_job_dir(dir: &Path) -> bool {
    dir.join("status.json").is_file() || dir.join("handoff").join("manifest.json").is_file()
}

/// Start `runner task` for one task file, in its own directory under `root`.
/// The directory is decided here, before the spawn, so the board can adopt it
/// without guessing; the runner's own chatter goes to `runner.log` beside the
/// stream it writes.
fn start(
    runner: &Path,
    task: &Path,
    root: &Path,
    now_s: i64,
) -> Result<(String, std::process::Child), String> {
    let name: String = label(task)
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let out = root.join(format!("{name}-{now_s}"));
    std::fs::create_dir_all(&out)
        .map_err(|error| format!("creating {}: {error}", out.display()))?;
    let log = std::fs::File::create(out.join("runner.log"))
        .map_err(|error| format!("writing {}: {error}", out.join("runner.log").display()))?;
    let stderr = log
        .try_clone()
        .map_err(|error| format!("writing {}: {error}", out.join("runner.log").display()))?;
    let spawned = std::process::Command::new(runner)
        .arg("task")
        .arg("--task")
        .arg(task)
        .arg("--out")
        .arg(&out)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(log))
        .stderr(std::process::Stdio::from(stderr))
        .spawn();
    match spawned {
        Ok(child) => Ok((name, child)),
        Err(error) => {
            let _ = std::fs::remove_dir_all(&out);
            Err(format!("could not start {}: {error}", runner.display()))
        }
    }
}

/// How a finished child reads in the header: 0 is a run whose verify passed,
/// and anything else is a runner that reported, or never got that far.
fn exit_label(exit: std::process::ExitStatus) -> String {
    match exit.code() {
        Some(0) => "finished".to_owned(),
        Some(code) => format!("exited {code}"),
        None => "stopped".to_owned(),
    }
}

/// What a watcher can do with a job. Both actions are the viewer's own; the
/// runner is the only thing that starts, stops or re-runs one.
fn live_actions(_job: &Job) -> ActionSet {
    [
        Some(Action::filled("Reveal")),
        Some(Action::ghost("Refresh")),
    ]
}

fn watch_status(root: &Path, scan: &Scan) -> String {
    let mut text = format!(
        "watching {} · {} job{}",
        root.display(),
        scan.jobs.len(),
        if scan.jobs.len() == 1 { "" } else { "s" }
    );
    if !scan.problems.is_empty() {
        text.push_str(&format!(" · {} unreadable", scan.problems.len()));
    }
    text
}

fn problems_text(problems: &[jobs::Problem]) -> String {
    problems
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Open the job directory in the platform's file manager. A direct exec, no
/// shell: the path is a directory the caller chose, passed as one argument.
fn reveal(dir: &Path) -> std::io::Result<()> {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    };
    std::process::Command::new(opener).arg(dir).spawn()?;
    Ok(())
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{shared_prefix, start};
    use std::path::Path;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn the_shared_prefix_stops_at_a_word_boundary() {
        assert_eq!(
            shared_prefix(&names(&["chromium-surfaces-a", "chromium-surfaces-b"])),
            "chromium-surfaces-"
        );
        assert_eq!(
            shared_prefix(&names(&["chromium-surfaces-a", "chromium-input-b"])),
            "chromium-"
        );
    }

    #[test]
    fn a_prefix_that_is_not_a_word_boundary_is_not_stripped() {
        assert_eq!(shared_prefix(&names(&["touches", "touchpad"])), "");
        assert_eq!(shared_prefix(&names(&[])), "");
    }

    #[test]
    fn a_lone_task_keeps_its_whole_name() {
        assert_eq!(
            shared_prefix(&names(&["chromium-surfaces-duplicate-started"])),
            ""
        );
    }

    #[test]
    fn a_start_gets_its_own_directory_and_keeps_the_runner_output() {
        let root = std::env::temp_dir().join(format!("jobs-board-start-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("a root");
        let task = root.join("a-task.json");
        std::fs::write(&task, "{}").expect("a task file");

        // `/bin/echo` stands in for the runner: what it prints is the argv the
        // board would have given `rig-eval task`.
        let (name, mut child) =
            start(Path::new("/bin/echo"), &task, &root, 1_700_000_000).expect("a child");
        assert_eq!(name, "a-task");
        let out = root.join("a-task-1700000000");
        assert!(out.join("runner.log").is_file());

        assert!(child.wait().expect("the child").success());
        let log = std::fs::read_to_string(out.join("runner.log")).expect("the log");
        assert!(log.contains("task --task"));
        assert!(log.contains("a-task-1700000000"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
