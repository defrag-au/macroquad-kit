//! Jobs — the container-flow board, at a phone's and a desk's width.
//!
//! The board itself is `jobs_board`; this module supplies four fixture jobs and
//! the actions a sheet would offer. The field values are the runner's own: the
//! done card's base, images, model, cost, patch size and effect count are the
//! one real run of `chromium-surfaces-duplicate-started`, and its elapsed time
//! and the other three cards are the states a board must draw, not
//! measurements. The reader's tests use the same shape under
//! `ui/jobs/tests/fixtures/root`.
//!
//! The actions are the design's, not the viewer's: the live binary offers
//! `Reveal` and `Refresh`, because a watcher cannot stop or re-run a job the
//! runner owns. These are what a host with that authority would put there. The
//! picker is the one write a live board does have, when it is given a runner.
//!
//! The done card's cost is the recorded run's; its token counts and elapsed
//! time are the fixture's, so the usage rows have a shape to draw. The reader
//! fills the same fields from `status.json`'s usage object, or from the agent
//! stream while a run is still going. The picker's task names are the fixture
//! jobs' own — labels for a screen, not files on disk.

use std::path::PathBuf;

use jobs::{Effects, Image, Job, Patch, Phase, StepState, Usage, VerifyStep};
use jobs_board::{Action, ActionSet, Board, BoardEvent, BoardFrame, Launch};
use macroquad::prelude::*;
use macroquad_widgets::{Gesture, Painter};

/// A phone's width, where the board is a single column.
const PHONE_W: f32 = 420.0;
const PHONE_H: f32 = 760.0;
/// A desk's width, where it is a grid of two.
const DESK_W: f32 = 900.0;
const DESK_H: f32 = 620.0;

const TASK_BASE: &str = "7045e3721ce4d05b93497d8eca0087bf2388089a";
const AGENT_IMAGE: &str = "playbook-eval/rig-eval:local";
const AGENT_DIGEST: &str =
    "sha256:ba4eb717a58edace149255bfc033892339fad00811d7861028d18c9b953f0286";
const VERIFY_IMAGE: &str = "playbook-eval/rust-verify:1.99";
const VERIFY_DIGEST: &str =
    "sha256:b700827c571dd4a977c02a6beebf52f1c622030064ab8aee1389c12b93f7f1a3";
const MODEL: &str = "deepseek/deepseek-flash";
const TEST_COMMAND: &str = "cargo test --locked -p surface -p surface-input";
const FORMAT_COMMAND: &str = "cargo fmt --all -- --check";

/// Which board a story shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobsView {
    /// The four cards, at a phone's width.
    Board,
    /// The same board at a desk's width: two columns.
    BoardDesk,
    /// A live run's sheet: the verify steps not started, and the actions that
    /// belong to work in progress.
    SheetRunning,
    /// The finished run's sheet, open and settled.
    SheetDone,
    /// The failed run's sheet, open and settled.
    SheetFailed,
    /// The task picker, open over the board.
    LauncherSheet,
    /// Tap a card for its sheet; the pills report what they would ask a host
    /// to do.
    Live,
}

pub struct JobsStory {
    view: JobsView,
    jobs: Vec<Job>,
    /// The task names the picker offers, in the order a host listed them.
    launches: Vec<Launch>,
    board: Board,
    last: Option<String>,
}

impl JobsStory {
    pub fn new(view: JobsView) -> Self {
        let jobs = fixtures();
        let board = match view {
            JobsView::SheetRunning => Board::opened_on(&jobs, Phase::Running),
            JobsView::SheetDone => Board::opened_on(&jobs, Phase::Done),
            JobsView::SheetFailed => Board::opened_on(&jobs, Phase::Failed),
            JobsView::LauncherSheet => Board::opened_on_launcher(),
            _ => Board::new(),
        };
        let launches = [
            "duplicate-started",
            "touch-origin",
            "wheel-points",
            "pointer-normalise",
        ]
        .into_iter()
        .map(|name| Launch {
            name: name.to_owned(),
        })
        .collect();
        Self {
            view,
            jobs,
            launches,
            board,
            last: None,
        }
    }

    /// The size this story is drawn for, in points.
    pub fn canvas(&self) -> Vec2 {
        match self.view {
            JobsView::BoardDesk => vec2(DESK_W, DESK_H),
            _ => vec2(PHONE_W, PHONE_H),
        }
    }

    pub fn draw(&mut self, p: &Painter, area: Rect, g: &Gesture, dt: f32) {
        let canvas = self.canvas();
        let stage = Rect::new(area.x, area.y, canvas.x.min(area.w), canvas.y.min(area.h));
        // A review capture is a picture, not an interactive surface: taps would
        // only change what is being reviewed. The live story takes them.
        let gesture = if self.view == JobsView::Live {
            *g
        } else {
            Gesture { tap: None, ..*g }
        };
        let frame = BoardFrame {
            area: stage,
            gesture: &gesture,
            dt,
            // The story's clock is the one `--at` freezes; the fixtures are
            // written in its seconds.
            now_s: p.time() as i64,
            jobs: &self.jobs,
            status: self.last.as_deref(),
            actions: story_actions,
            launches: &self.launches,
        };
        if let Some(event) = self.board.draw(p, frame) {
            self.last = Some(match event {
                BoardEvent::Opened { task } => format!("opened {task}"),
                BoardEvent::Action { task, index } => {
                    let label = self
                        .jobs
                        .iter()
                        .find(|job| job.task == task)
                        .and_then(|job| story_actions(job)[index].map(|action| action.label))
                        .unwrap_or("action");
                    format!("{task}: {label}")
                }
                BoardEvent::Launch { index } => {
                    let name = self
                        .launches
                        .get(index)
                        .map(|launch| launch.name.as_str())
                        .unwrap_or("task");
                    format!("start {name}")
                }
            });
        }
    }
}

/// What a sheet offers a host that owns the runner.
fn story_actions(job: &Job) -> ActionSet {
    match job.phase {
        Phase::Running | Phase::Verifying => {
            [Some(Action::ghost("Log")), Some(Action::danger("Stop"))]
        }
        Phase::Done => [
            Some(Action::filled("Re-verify")),
            Some(Action::ghost("Apply patch")),
        ],
        Phase::Failed => [Some(Action::filled("Resume")), Some(Action::ghost("Log"))],
    }
}

fn fixtures() -> Vec<Job> {
    vec![
        Job {
            phase: Phase::Running,
            started_s: Some(-83),
            verify: verify(StepState::Pending, StepState::Pending),
            ..job("chromium-surfaces-touch-origin")
        },
        Job {
            phase: Phase::Verifying,
            started_s: Some(-312),
            verify: verify(StepState::Ok, StepState::Pending),
            ..job("chromium-surfaces-wheel-points")
        },
        Job {
            phase: Phase::Done,
            started_s: Some(-161),
            finished_s: Some(0),
            usage: Some(Usage {
                model_calls: 3,
                input_tokens: 12_400,
                output_tokens: 800,
                cached_input_tokens: 9_000,
                cost_usd: 0.0022,
            }),
            patch: Some(Patch {
                bytes: 1_765,
                applies_on_host: true,
            }),
            effects: Some(Effects {
                events: 24,
                bytes: 134 * 1_024,
            }),
            verify: verify(StepState::Ok, StepState::Ok),
            ..job("chromium-surfaces-duplicate-started")
        },
        Job {
            phase: Phase::Failed,
            started_s: Some(-208),
            finished_s: Some(0),
            verify: verify(StepState::Ok, StepState::Failed),
            ..job("chromium-surfaces-pointer-normalise")
        },
    ]
}

/// The four jobs share their repository and pipeline; a fixture starts from
/// these and overrides what its moment adds.
fn job(task: &str) -> Job {
    Job {
        task: task.to_owned(),
        repo: "chromium-surfaces".to_owned(),
        base: TASK_BASE.to_owned(),
        phase: Phase::Running,
        started_s: None,
        updated_s: None,
        finished_s: None,
        image: Some(Image {
            reference: AGENT_IMAGE.to_owned(),
            digest: AGENT_DIGEST.to_owned(),
        }),
        verify_image: Some(Image {
            reference: VERIFY_IMAGE.to_owned(),
            digest: VERIFY_DIGEST.to_owned(),
        }),
        model: Some(MODEL.to_owned()),
        session: None,
        usage: None,
        patch: None,
        effects: None,
        verify: Vec::new(),
        dir: PathBuf::from(task),
    }
}

fn verify(tests: StepState, format: StepState) -> Vec<VerifyStep> {
    vec![
        VerifyStep {
            index: 1,
            command: TEST_COMMAND.to_owned(),
            label: "tests · surface + surface-input".to_owned(),
            state: tests,
        },
        VerifyStep {
            index: 2,
            command: FORMAT_COMMAND.to_owned(),
            label: "format".to_owned(),
            state: format,
        },
    ]
}
