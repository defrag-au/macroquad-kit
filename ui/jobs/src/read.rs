//! Reading a job directory, and a root of them.
//!
//! The two handoff files are read separately and joined here so a caller never
//! has to know which half a fact is in. A third file, the agent's raw stream, is
//! read only for the usage a run has not recorded yet. See the
//! [crate docs](crate) for why `status.json` wins where both have a field.

use std::fmt;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::{
    Effects, Image, Job, Patch, Phase, StepState, TimeError, Usage, VerifyStep, parse_utc,
};

/// Every job under a root, and every directory that could not be read.
#[derive(Debug, Default)]
pub struct Scan {
    pub jobs: Vec<Job>,
    /// Directories with a job file that did not parse. They are reported, not
    /// rendered: a card for a job whose phase nobody can name would be a lie.
    pub problems: Vec<Problem>,
}

/// One directory that looks like a job and cannot be read.
#[derive(Debug)]
pub struct Problem {
    pub path: PathBuf,
    pub error: ReadError,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.error)
    }
}

impl std::error::Error for Problem {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.error.source()
    }
}

/// Why a job directory could not be read.
#[derive(Debug)]
pub enum ReadError {
    Io(std::io::Error),
    Json(serde_json::Error),
    /// A phase this crate does not know: a status from a newer runner.
    UnknownPhase(String),
    /// A verify status this crate does not know.
    UnknownVerifyStatus(String),
    /// A timestamp that is not the runner's `YYYY-MM-DDTHH:MM:SSZ`.
    Time {
        value: String,
        why: String,
    },
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "could not read the artefacts: {e}"),
            Self::Json(e) => write!(f, "could not parse the artefacts: {e}"),
            Self::UnknownPhase(phase) => write!(f, "unknown phase {phase:?}"),
            Self::UnknownVerifyStatus(status) => write!(f, "unknown verify status {status:?}"),
            Self::Time { value, why } => write!(f, "bad timestamp {value:?}: {why}"),
        }
    }
}

impl std::error::Error for ReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Json(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for ReadError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for ReadError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

#[derive(Deserialize)]
struct StatusFile {
    phase: String,
    task: String,
    #[serde(default)]
    repo: Option<String>,
    #[serde(default)]
    base: Option<String>,
    #[serde(default)]
    image: Option<ImageFile>,
    #[serde(default)]
    verify_image: Option<ImageFile>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    session: Option<String>,
    #[serde(default)]
    started_at: Option<String>,
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default)]
    verify: Vec<VerifyFile>,
    #[serde(default)]
    usage: Option<UsageFile>,
}

#[derive(Deserialize)]
struct ManifestFile {
    task: String,
    repo: String,
    base: String,
    image: ImageFile,
    verify_image: ImageFile,
    model: String,
    #[serde(default)]
    session: Option<String>,
    #[serde(default)]
    verify: Vec<VerifyFile>,
    verify_passed: bool,
    #[serde(default)]
    patch: Option<PatchFile>,
    #[serde(default)]
    effects: Option<String>,
    #[serde(default)]
    usage: Option<UsageFile>,
}

#[derive(Clone, Deserialize)]
struct ImageFile {
    reference: String,
    digest: String,
}

impl From<ImageFile> for Image {
    fn from(value: ImageFile) -> Self {
        Self {
            reference: value.reference,
            digest: value.digest,
        }
    }
}

#[derive(Deserialize)]
struct PatchFile {
    bytes: u64,
    applies_on_host: bool,
}

#[derive(Deserialize)]
struct VerifyFile {
    index: u32,
    status: String,
    command: String,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
struct UsageFile {
    #[serde(default)]
    model_calls: u64,
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cached_input_tokens: u64,
    #[serde(default)]
    cost_usd: f64,
}

impl From<UsageFile> for Usage {
    fn from(value: UsageFile) -> Self {
        Self {
            model_calls: value.model_calls,
            input_tokens: value.input_tokens,
            output_tokens: value.output_tokens,
            cached_input_tokens: value.cached_input_tokens,
            cost_usd: value.cost_usd,
        }
    }
}

/// One line of `rig -p --json`. Only the usage event is read here; everything
/// else is [`AgentEvent::Other`].
#[derive(Deserialize)]
#[serde(tag = "type")]
enum AgentEvent {
    #[serde(rename = "usage")]
    Usage {
        #[serde(default)]
        usage: Option<StreamUsage>,
    },
    #[serde(other)]
    Other,
}

/// The stream's usage shape: its cost is `cost.total`, where the handoff's
/// recorded usage is the flat `cost_usd` sum the runner writes.
#[derive(Clone, Copy, Debug, Default, Deserialize)]
struct StreamUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cached_input_tokens: u64,
    #[serde(default)]
    cost: Option<StreamCost>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
struct StreamCost {
    #[serde(default)]
    total: f64,
}

/// Read one job directory. `Ok(None)` when it is not a job directory at all:
/// neither a status nor a manifest.
pub fn read_job(dir: &Path) -> Result<Option<Job>, ReadError> {
    let status_path = dir.join("status.json");
    let manifest_path = dir.join("handoff").join("manifest.json");
    let status: Option<StatusFile> = if status_path.is_file() {
        Some(read_json(&status_path)?)
    } else {
        None
    };
    let manifest: Option<ManifestFile> = if manifest_path.is_file() {
        Some(read_json(&manifest_path)?)
    } else {
        None
    };
    match (status, manifest) {
        (None, None) => Ok(None),
        (Some(status), manifest) => from_status(status, manifest.as_ref(), dir).map(Some),
        (None, Some(manifest)) => from_manifest(manifest, dir).map(Some),
    }
}

/// Every job directory directly under `root`, or `root` itself when it is one.
///
/// A directory with neither a status nor a manifest is skipped quietly: a root
/// collects whatever it is given, and a stray `env` or `repo` directory is not
/// a broken job. A directory with a job file that does not parse is a
/// [`Problem`].
pub fn scan(root: &Path) -> Scan {
    let mut scan = Scan::default();
    if is_job_dir(root) {
        match read_job(root) {
            Ok(Some(job)) => scan.jobs.push(job),
            Ok(None) => {}
            Err(error) => scan.problems.push(Problem {
                path: root.to_path_buf(),
                error,
            }),
        }
        return scan;
    }
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) => {
            scan.problems.push(Problem {
                path: root.to_path_buf(),
                error: ReadError::Io(error),
            });
            return scan;
        }
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    for dir in dirs {
        if !is_job_dir(&dir) {
            continue;
        }
        match read_job(&dir) {
            Ok(Some(job)) => scan.jobs.push(job),
            Ok(None) => {}
            Err(error) => scan.problems.push(Problem { path: dir, error }),
        }
    }
    // Newest work first, grouped by what a reader acts on: running and
    // verifying, then failed, then done. An unknown start (a manifest-only
    // directory) sorts after the dated runs in its group.
    scan.jobs.sort_by(|a, b| {
        rank(a.phase)
            .cmp(&rank(b.phase))
            .then_with(|| b.started_s.cmp(&a.started_s))
            .then_with(|| a.task.cmp(&b.task))
    });
    scan
}

fn rank(phase: Phase) -> u8 {
    match phase {
        Phase::Running | Phase::Verifying => 0,
        Phase::Failed => 1,
        Phase::Done => 2,
    }
}

fn is_job_dir(dir: &Path) -> bool {
    dir.join("status.json").is_file() || dir.join("handoff").join("manifest.json").is_file()
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, ReadError> {
    let text = fs::read_to_string(path)?;
    serde_json::from_str(&text).map_err(ReadError::Json)
}

fn from_status(
    status: StatusFile,
    manifest: Option<&ManifestFile>,
    dir: &Path,
) -> Result<Job, ReadError> {
    let phase =
        Phase::parse(&status.phase).ok_or_else(|| ReadError::UnknownPhase(status.phase.clone()))?;
    let verify = if status.verify.is_empty() {
        manifest
            .map(|m| steps(&m.verify))
            .transpose()?
            .unwrap_or_default()
    } else {
        steps(&status.verify)?
    };
    let started_s = status.started_at.as_deref().map(parse_time).transpose()?;
    let updated_s = status.updated_at.as_deref().map(parse_time).transpose()?;
    let finished_s = if phase.active() { None } else { updated_s };
    let patch = manifest.and_then(|m| m.patch.as_ref()).map(|p| Patch {
        bytes: p.bytes,
        applies_on_host: p.applies_on_host,
    });
    let effects = match manifest.and_then(|m| m.effects.as_deref()) {
        Some(name) => read_effects(&dir.join("handoff").join(name))?,
        None => None,
    };
    let usage = match status.usage.or(manifest.and_then(|m| m.usage)) {
        Some(usage) => Some(usage.into()),
        None => read_agent_usage(&dir.join("agent.jsonl"))?,
    };
    let image = status
        .image
        .map(Image::from)
        .or_else(|| manifest.map(|m| m.image.clone().into()));
    let verify_image = status
        .verify_image
        .map(Image::from)
        .or_else(|| manifest.map(|m| m.verify_image.clone().into()));
    Ok(Job {
        task: status.task,
        repo: repo_name(
            status
                .repo
                .as_deref()
                .or_else(|| manifest.map(|m| m.repo.as_str()))
                .unwrap_or(""),
        ),
        base: status
            .base
            .or_else(|| manifest.map(|m| m.base.clone()))
            .unwrap_or_default(),
        phase,
        started_s,
        updated_s,
        finished_s,
        image,
        verify_image,
        model: status.model.or_else(|| manifest.map(|m| m.model.clone())),
        session: status
            .session
            .or_else(|| manifest.and_then(|m| m.session.clone())),
        usage,
        patch,
        effects,
        verify,
        dir: dir.to_path_buf(),
    })
}

fn from_manifest(manifest: ManifestFile, dir: &Path) -> Result<Job, ReadError> {
    let phase = if manifest.verify_passed {
        Phase::Done
    } else {
        Phase::Failed
    };
    let effects = match manifest.effects.as_deref() {
        Some(name) => read_effects(&dir.join("handoff").join(name))?,
        None => None,
    };
    let usage = match manifest.usage {
        Some(usage) => Some(usage.into()),
        None => read_agent_usage(&dir.join("agent.jsonl"))?,
    };
    Ok(Job {
        task: manifest.task,
        repo: repo_name(&manifest.repo),
        base: manifest.base,
        phase,
        started_s: None,
        updated_s: None,
        finished_s: None,
        image: Some(manifest.image.into()),
        verify_image: Some(manifest.verify_image.into()),
        model: Some(manifest.model),
        session: manifest.session,
        usage,
        patch: manifest.patch.map(|p| Patch {
            bytes: p.bytes,
            applies_on_host: p.applies_on_host,
        }),
        effects,
        verify: steps(&manifest.verify)?,
        dir: dir.to_path_buf(),
    })
}

fn steps(files: &[VerifyFile]) -> Result<Vec<VerifyStep>, ReadError> {
    files
        .iter()
        .map(|file| {
            let state = StepState::parse(&file.status)
                .ok_or_else(|| ReadError::UnknownVerifyStatus(file.status.clone()))?;
            Ok(VerifyStep {
                index: file.index,
                command: file.command.clone(),
                // A read job has no curated label: the command is what the
                // task named the check, and the sheet shows it whole at its
                // small size.
                label: file.command.clone(),
                state,
            })
        })
        .collect()
}

/// A repository's name from what the manifest records: the runner writes the
/// absolute checkout, and a board shows its last segment.
fn repo_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| path.to_owned())
}

fn parse_time(text: &str) -> Result<i64, ReadError> {
    parse_utc(text).map_err(|why: TimeError| ReadError::Time {
        value: text.to_owned(),
        why: why.to_string(),
    })
}

/// The effect log's size and event count. A missing file is no effects rather
/// than a broken job: everything else in the handoff is still readable.
fn read_effects(path: &Path) -> Result<Option<Effects>, ReadError> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(Effects {
            events: text.lines().filter(|line| !line.trim().is_empty()).count(),
            bytes: text.len() as u64,
        })),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ReadError::Io(error)),
    }
}

/// The usage an agent stream already carries. The handoff's usage is the
/// record; this is what a board reads while the agent is still writing, and it
/// skips a line that does not parse because the last one may be mid-write.
fn read_agent_usage(path: &Path) -> Result<Option<Usage>, ReadError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(ReadError::Io(error)),
    };
    let mut usage = Usage::default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(event) = serde_json::from_str::<AgentEvent>(line) else {
            continue;
        };
        let AgentEvent::Usage { usage: spent } = event else {
            continue;
        };
        usage.model_calls += 1;
        if let Some(spent) = spent {
            usage.input_tokens += spent.input_tokens;
            usage.output_tokens += spent.output_tokens;
            usage.cached_input_tokens += spent.cached_input_tokens;
            usage.cost_usd += spent.cost.map(|cost| cost.total).unwrap_or_default();
        }
    }
    Ok((usage.model_calls > 0).then_some(usage))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repository_is_shown_by_its_name() {
        assert_eq!(
            repo_name("/Users/damo/code/defrag/chromium-surfaces"),
            "chromium-surfaces"
        );
        assert_eq!(repo_name("chromium-surfaces"), "chromium-surfaces");
        assert_eq!(repo_name(""), "");
    }
}
