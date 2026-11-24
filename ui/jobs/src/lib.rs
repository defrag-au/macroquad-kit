//! The container-flow job artefacts, read into one typed board.
//!
//! A run leaves a directory the runner owns: `status.json` while it is going,
//! then `handoff/manifest.json`, the patch, the verify logs and a copy of
//! `effects.jsonl`. This crate turns one such directory into a [`Job`], and a
//! root of them into a [`Scan`], with no renderer and no dependency on
//! macroquad. The board, the storybook fixture and any future CLI read the same
//! types.
//!
//! The two files answer different halves, and the reader joins them:
//!
//! - **`status.json`** is the live half: the phase, the task's identity, the
//!   whole verify plan (`pending` until each command runs), timestamps, and the
//!   session once one exists. It is rewritten atomically as the run moves.
//! - **`handoff/manifest.json`** is the finished half: the patch and whether it
//!   applies, the effect log's name, and the final verify outcome. It exists
//!   only after the run is over.
//!
//! Where a field is in both, `status.json` wins: it is the newer file. A
//! directory with only a manifest (an older run, or one whose status was
//! cleaned up) still reads, with its phase taken from `verify_passed`.
//!
//! Usage — model calls, tokens and cost — is summed from the agent stream's
//! `usage` events. The runner writes the sum into both files once the agent
//! stops; while a run is still going, and for a directory with no recorded
//! usage, the reader sums `agent.jsonl` itself, so a board can show the spend
//! as it happens. No usage events is `None`, not a zero cost: an unpriced run is
//! not a free one.

mod read;

use std::fmt;

pub use read::{Problem, ReadError, Scan, read_job, scan};

/// Which moment of a run a job is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Preparing the snapshot or running the agent. The runner's `snapshot`
    /// phase is here too: it lasts as long as the archive takes, and a board
    /// that showed it separately would flicker.
    Running,
    Verifying,
    Done,
    Failed,
}

impl Phase {
    /// The runner's phase name. `None` for a name this crate does not know —
    /// a status from a newer runner, which the caller should report rather
    /// than render as a phase it is not.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "snapshot" | "running" => Some(Self::Running),
            "verifying" => Some(Self::Verifying),
            "done" => Some(Self::Done),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Verifying => "verifying",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }

    /// Whether the run is still moving.
    pub fn active(self) -> bool {
        matches!(self, Self::Running | Self::Verifying)
    }
}

/// One verify command's outcome so far.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepState {
    Pending,
    Ok,
    Failed,
}

impl StepState {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "pending" => Some(Self::Pending),
            "ok" => Some(Self::Ok),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Pending => "waiting",
            Self::Ok => "passed",
            Self::Failed => "failed",
        }
    }
}

/// One verify command in the task's plan.
#[derive(Clone, Debug, PartialEq)]
pub struct VerifyStep {
    pub index: u32,
    /// The command as the task named it.
    pub command: String,
    /// How a sheet names it: the command itself when the host has no shorter
    /// name, a chosen label where it does.
    pub label: String,
    pub state: StepState,
}

/// An image by reference and digest: what a result is reproducible from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub reference: String,
    pub digest: String,
}

/// The patch the runner produced, and whether the host can apply it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Patch {
    pub bytes: u64,
    pub applies_on_host: bool,
}

/// The copy of the session's effect log the handoff keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Effects {
    pub events: usize,
    pub bytes: u64,
}

/// What the run's model calls spent. Summed from the agent stream's `usage`
/// events; `None` when the stream carried none.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Usage {
    /// One per usage event, compactions included.
    pub model_calls: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// The part of `input_tokens` the provider read from its prompt cache.
    pub cached_input_tokens: u64,
    pub cost_usd: f64,
}

/// One container task, as a board draws it.
#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub task: String,
    /// The repository's name, not its host path: a manifest records the
    /// absolute checkout, and a board shows `chromium-surfaces`.
    pub repo: String,
    /// The base commit; a renderer shortens it.
    pub base: String,
    pub phase: Phase,
    /// Seconds since the Unix epoch. `None` for a manifest-only directory,
    /// whose artefact does not carry a start time.
    pub started_s: Option<i64>,
    /// The last status write, for a host that reads how fresh it is.
    pub updated_s: Option<i64>,
    /// When a finished run stopped; `None` while it is still going.
    pub finished_s: Option<i64>,
    pub image: Option<Image>,
    pub verify_image: Option<Image>,
    pub model: Option<String>,
    pub session: Option<String>,
    /// The run's spend, from the handoff when it has one and from the agent
    /// stream while a run is still going.
    pub usage: Option<Usage>,
    pub patch: Option<Patch>,
    pub effects: Option<Effects>,
    pub verify: Vec<VerifyStep>,
    /// The directory these artefacts were read from: what a host reveals.
    pub dir: std::path::PathBuf,
}

impl Job {
    /// The task name without its repository prefix:
    /// `chromium-surfaces-duplicate-started` on `chromium-surfaces` reads
    /// `duplicate-started`.
    pub fn short_task(&self) -> &str {
        if self.repo.is_empty() {
            return self.task.as_str();
        }
        self.task
            .strip_prefix(self.repo.as_str())
            .and_then(|rest| rest.strip_prefix('-'))
            .unwrap_or(self.task.as_str())
    }

    /// How long the run has taken: its own duration when finished, otherwise
    /// up to `now_s`. `None` when the artefacts carry no start time.
    pub fn elapsed_s(&self, now_s: i64) -> Option<i64> {
        let started = self.started_s?;
        Some(
            self.finished_s
                .unwrap_or(now_s)
                .saturating_sub(started)
                .max(0),
        )
    }

    /// How many verify commands have passed.
    pub fn passed(&self) -> usize {
        self.verify
            .iter()
            .filter(|s| s.state == StepState::Ok)
            .count()
    }
}

/// A UTC stamp that cannot be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TimeError {
    /// Not `YYYY-MM-DDTHH:MM:SSZ`.
    Format,
    /// A field is not in its range (month 13, February 30, hour 24).
    Range,
}

impl fmt::Display for TimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Format => f.write_str("expected YYYY-MM-DDTHH:MM:SSZ"),
            Self::Range => f.write_str("a date or time field is out of range"),
        }
    }
}

/// Seconds since the Unix epoch for the runner's `%Y-%m-%dT%H:%M:%SZ`.
///
/// Hand-written rather than a date-time crate: the format is fixed, the crate
/// would be a dependency of every UI that shows a job, and the two functions
/// below are the whole calendar a UTC stamp needs.
pub(crate) fn parse_utc(text: &str) -> Result<i64, TimeError> {
    let b = text.as_bytes();
    let separators = b.len() == 20
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':'
        && b[19] == b'Z';
    if !separators {
        return Err(TimeError::Format);
    }
    let num = |from: usize| -> Result<i64, TimeError> {
        let mut value = 0;
        for &c in &b[from..from + 2] {
            if !c.is_ascii_digit() {
                return Err(TimeError::Format);
            }
            value = value * 10 + i64::from(c - b'0');
        }
        Ok(value)
    };
    let year = {
        let mut value = 0;
        for &c in &b[0..4] {
            if !c.is_ascii_digit() {
                return Err(TimeError::Format);
            }
            value = value * 10 + i64::from(c - b'0');
        }
        value
    };
    let month = num(5)?;
    let day = num(8)?;
    let hour = num(11)?;
    let minute = num(14)?;
    let second = num(17)?;
    let in_range = (1..=12).contains(&month)
        && (1..=days_in_month(year, month as u32)).contains(&day)
        && hour <= 23
        && minute <= 59
        && second <= 59;
    if !in_range {
        return Err(TimeError::Range);
    }
    Ok(days_from_civil(year, month as u32, day as u32) * 86_400
        + hour * 3_600
        + minute * 60
        + second)
}

fn days_in_month(year: i64, month: u32) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Days from 1970-01-01 to `year-month-day`, in the proleptic Gregorian
/// calendar (Howard Hinnant's `days_from_civil`).
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (i64::from(month) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_utc_stamp_is_seconds_since_the_epoch() {
        assert_eq!(parse_utc("1970-01-01T00:00:00Z"), Ok(0));
        assert_eq!(parse_utc("1970-01-02T00:00:00Z"), Ok(86_400));
        assert_eq!(parse_utc("2000-03-01T00:00:00Z"), Ok(951_868_800));
        // The first successful run's recorded start.
        assert_eq!(parse_utc("2026-11-22T13:45:39Z"), Ok(1_795_355_139));
    }

    #[test]
    fn leap_days_are_the_gregorian_ones() {
        assert!(parse_utc("2000-02-29T00:00:00Z").is_ok());
        assert!(
            parse_utc("1900-02-29T00:00:00Z").is_err(),
            "1900 is not a leap year"
        );
        assert!(parse_utc("2024-02-29T00:00:00Z").is_ok());
    }

    #[test]
    fn a_stamp_that_is_not_the_runner_format_is_refused() {
        for text in [
            "2026-11-22 13:45:39Z",
            "2026-11-22T13:45:39",
            "2026-11-22T13:45:39z",
            "2026-11-22T13:45:39+00:00",
            "2026-1-22T13:45:39Z",
            "not a timestamp at all!",
        ] {
            assert_eq!(parse_utc(text), Err(TimeError::Format), "{text}");
        }
    }

    #[test]
    fn a_field_out_of_range_is_refused() {
        for text in [
            "2026-13-01T00:00:00Z",
            "2026-00-01T00:00:00Z",
            "2026-02-30T00:00:00Z",
            "2026-11-00T00:00:00Z",
            "2026-11-22T24:00:00Z",
            "2026-11-22T13:60:00Z",
            "2026-11-22T13:45:60Z",
        ] {
            assert_eq!(parse_utc(text), Err(TimeError::Range), "{text}");
        }
    }

    fn job(task: &str, repo: &str) -> Job {
        Job {
            task: task.into(),
            repo: repo.into(),
            base: String::new(),
            phase: Phase::Running,
            started_s: None,
            updated_s: None,
            finished_s: None,
            image: None,
            verify_image: None,
            model: None,
            session: None,
            usage: None,
            patch: None,
            effects: None,
            verify: Vec::new(),
            dir: std::path::PathBuf::new(),
        }
    }

    #[test]
    fn the_short_task_drops_the_repository_prefix() {
        assert_eq!(
            job("chromium-surfaces-duplicate-started", "chromium-surfaces").short_task(),
            "duplicate-started"
        );
        // A task that merely starts with the repo's letters is left whole.
        assert_eq!(
            job("chromium-surfaces2-thing", "chromium-surfaces").short_task(),
            "chromium-surfaces2-thing"
        );
        assert_eq!(job("unrelated", "").short_task(), "unrelated");
    }

    #[test]
    fn elapsed_is_the_run_or_the_clock_and_never_negative() {
        let mut j = job("task", "repo");
        assert_eq!(j.elapsed_s(100), None, "no start time, no duration");
        j.started_s = Some(10);
        assert_eq!(j.elapsed_s(100), Some(90));
        j.finished_s = Some(40);
        assert_eq!(
            j.elapsed_s(100),
            Some(30),
            "a finished run keeps its own end"
        );
        j.started_s = Some(120);
        assert_eq!(
            j.elapsed_s(100),
            Some(0),
            "a clock behind the start is not negative"
        );
    }
}
