//! The reader against the runner's real artefact shapes.
//!
//! The fixture root is a small board: a running job, a verifying one, a failed
//! one, a finished one with its handoff, a manifest-only directory (an older
//! run), and one status from a hypothetical newer runner that this crate must
//! report rather than guess at.

use std::path::{Path, PathBuf};

use jobs::{Effects, Patch, Phase, StepState, Usage, read_job, scan};

/// The moment the fixture statuses were written around.
const NOW: i64 = 1_795_355_100; // 2026-11-22T13:45:00Z

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/root")
}

fn by_task<'a>(scan: &'a jobs::Scan, task: &str) -> &'a jobs::Job {
    scan.jobs
        .iter()
        .find(|job| job.task == task)
        .unwrap_or_else(|| {
            panic!(
                "no {task} in {:?}",
                scan.jobs.iter().map(|j| &j.task).collect::<Vec<_>>()
            )
        })
}

#[test]
fn a_root_of_jobs_reads_into_a_sorted_board() {
    let scan = scan(&root());
    let tasks: Vec<&str> = scan.jobs.iter().map(|job| job.task.as_str()).collect();
    assert_eq!(
        tasks,
        [
            "chromium-surfaces-touch-origin",
            "chromium-surfaces-wheel-points",
            "chromium-surfaces-pointer-normalise",
            "chromium-surfaces-duplicate-started",
            "manifest-only-job",
        ],
        "active first, then failed, then done; newest start first in a group"
    );
    assert_eq!(scan.problems.len(), 1, "{:?}", scan.problems);
    assert!(
        scan.problems[0]
            .to_string()
            .contains("unknown phase \"paused\""),
        "{}",
        scan.problems[0]
    );
}

#[test]
fn a_live_status_carries_its_identity_and_its_whole_verify_plan() {
    let scan = scan(&root());
    let running = by_task(&scan, "chromium-surfaces-touch-origin");
    assert_eq!(running.phase, Phase::Running);
    assert_eq!(running.repo, "chromium-surfaces");
    assert_eq!(running.base, "7045e3721ce4d05b93497d8eca0087bf2388089a");
    assert_eq!(running.elapsed_s(NOW), Some(83));
    assert_eq!(
        running.image.as_ref().map(|image| image.reference.as_str()),
        Some("playbook-eval/rig-eval:local")
    );
    assert_eq!(running.model.as_deref(), Some("deepseek/deepseek-flash"));
    assert_eq!(
        running
            .verify
            .iter()
            .map(|step| step.state)
            .collect::<Vec<_>>(),
        [StepState::Pending, StepState::Pending]
    );
    assert_eq!(
        running.verify[0].label, "cargo test --locked -p surface -p surface-input",
        "a read job has no curated label: the command is the name"
    );
    let usage = running
        .usage
        .expect("a running job's spend comes from its agent stream");
    assert_eq!(usage.model_calls, 2);
    assert_eq!(usage.input_tokens, 8_000);
    assert_eq!(usage.output_tokens, 450);
    assert_eq!(usage.cached_input_tokens, 5_500);
    assert!(
        (usage.cost_usd - 0.0013).abs() < 1e-12,
        "the two usage events sum: {}",
        usage.cost_usd
    );
}

#[test]
fn a_verifying_job_shows_the_step_that_has_run() {
    let scan = scan(&root());
    let verifying = by_task(&scan, "chromium-surfaces-wheel-points");
    assert_eq!(verifying.phase, Phase::Verifying);
    assert_eq!(verifying.elapsed_s(NOW), Some(312));
    assert_eq!(
        verifying
            .verify
            .iter()
            .map(|step| step.state)
            .collect::<Vec<_>>(),
        [StepState::Ok, StepState::Pending]
    );
}

#[test]
fn a_finished_job_joins_its_status_and_its_handoff() {
    let scan = scan(&root());
    let done = by_task(&scan, "chromium-surfaces-duplicate-started");
    assert_eq!(done.phase, Phase::Done);
    assert_eq!(
        done.elapsed_s(0),
        Some(161),
        "a finished run keeps the duration it recorded"
    );
    assert_eq!(
        done.patch,
        Some(Patch {
            bytes: 1_765,
            applies_on_host: true
        })
    );
    assert_eq!(
        done.effects,
        Some(Effects {
            events: 3,
            bytes: 9
        })
    );
    assert_eq!(done.passed(), 2);
    assert_eq!(done.short_task(), "duplicate-started");
    assert_eq!(
        done.usage,
        Some(Usage {
            model_calls: 3,
            input_tokens: 12_400,
            output_tokens: 800,
            cached_input_tokens: 9_000,
            cost_usd: 0.0022,
        })
    );
}

#[test]
fn a_failed_job_keeps_the_check_that_failed() {
    let scan = scan(&root());
    let failed = by_task(&scan, "chromium-surfaces-pointer-normalise");
    assert_eq!(failed.phase, Phase::Failed);
    assert_eq!(failed.elapsed_s(0), Some(208));
    assert_eq!(
        failed
            .verify
            .iter()
            .map(|step| step.state)
            .collect::<Vec<_>>(),
        [StepState::Ok, StepState::Failed]
    );
    assert_eq!(failed.passed(), 1);
}

#[test]
fn a_manifest_alone_still_reads_a_job() {
    let scan = scan(&root());
    let old = by_task(&scan, "manifest-only-job");
    assert_eq!(old.phase, Phase::Done);
    assert_eq!(
        old.repo, "chromium-surfaces",
        "the absolute checkout is a name"
    );
    assert_eq!(old.started_s, None);
    assert_eq!(old.elapsed_s(NOW), None, "no start, no duration");
    assert_eq!(old.passed(), 2);
    assert_eq!(
        old.patch,
        Some(Patch {
            bytes: 0,
            applies_on_host: false
        })
    );
    assert_eq!(
        old.usage,
        Some(Usage {
            model_calls: 1,
            input_tokens: 2_000,
            output_tokens: 100,
            cached_input_tokens: 0,
            cost_usd: 0.0,
        })
    );
}

#[test]
fn one_directory_reads_on_its_own_and_a_plain_one_is_no_job() {
    let dir = root().join("duplicate-started");
    let job = read_job(&dir).unwrap().expect("a job");
    assert_eq!(job.dir, dir);
    assert!(read_job(&root()).unwrap().is_none(), "a root is not a job");
    assert!(
        read_job(&root().join("not-a-directory")).unwrap().is_none(),
        "no status, no manifest, no job"
    );
}
