//! The container-flow board: one card per job, its verify results and the
//! facts a handoff records, on one screen.
//!
//! The board knows nothing of files or processes. It draws [`jobs::Job`] values
//! and reports what was tapped; a host supplies the footer actions, the tasks
//! it can start, and what they mean. `macroquad-storybook` feeds it the design
//! fixtures, and the `jobs-board` binary feeds it the directories the runner
//! wrote plus the task files it may start.
//!
//! The state that lives across frames is the host-owned kind, not a widget's:
//! which card is open, the sheet's slide, the list's scroll, and the line the
//! host puts in the header.

use std::collections::BTreeSet;

use jobs::{Job, Phase, StepState, Usage};
use macroquad::prelude::*;
use macroquad_gamekit::with_clip;
use macroquad_widgets::theme::with_alpha;
use macroquad_widgets::{
    Align, Gesture, Painter, Pill, PillStyle, SURFACE_R, Scroll, ScrollInput, Sheet, SheetForm,
    SheetState, Spinner, TextSize, WellRow, draw_circle, edge, fill_rounded, pill_row, sheet_card,
    sheet_chrome, surface, well, well_height,
};

/// The title band: the word, the scope, the tally, the host's line.
const HEAD_H: f32 = 76.0;
const CARD_H: f32 = 88.0;
const GAP: f32 = 10.0;
const PAD: f32 = 14.0;
/// Between the verify panel and the run's own facts.
const DETAIL_GAP: f32 = 12.0;
/// Wider than this, the list is two columns.
const TWO_COLUMNS_AT: f32 = 640.0;

/// A footer action a host offers for a job. The board lays it out and reports
/// the tap; what it means is the host's, which is what lets the storybook echo
/// the actions a board *could* have while the live binary offers the two it
/// can actually do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Action {
    pub label: &'static str,
    pub style: PillStyle,
}

impl Action {
    pub fn filled(label: &'static str) -> Self {
        Self {
            label,
            style: PillStyle::Filled,
        }
    }

    pub fn ghost(label: &'static str) -> Self {
        Self {
            label,
            style: PillStyle::Ghost,
        }
    }

    pub fn danger(label: &'static str) -> Self {
        Self {
            label,
            style: PillStyle::Danger,
        }
    }
}

/// Up to two actions, in the order the sheet draws them.
pub type ActionSet = [Option<Action>; 2];

/// A task a host can start. The board draws the label and reports the index;
/// whether and how to start it is the host's, which is what keeps a renderer
/// free of processes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub name: String,
}

/// What the reader did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BoardEvent {
    /// A card was tapped and its sheet is opening.
    Opened { task: String },
    /// Footer action `index` (into the [`ActionSet`] the host returned) was
    /// tapped.
    Action { task: String, index: usize },
    /// Task `index` (into [`BoardFrame::launches`]) was tapped to start.
    Launch { index: usize },
}

/// Everything the board needs for one frame. `now_s` is the clock elapsed is
/// measured against: the wall's in the binary, the storybook's fixed clock in
/// a capture.
pub struct BoardFrame<'a> {
    pub area: Rect,
    pub gesture: &'a Gesture,
    pub dt: f32,
    pub now_s: i64,
    pub jobs: &'a [Job],
    /// The host's line under the tally: what it last did, or what it is
    /// watching.
    pub status: Option<&'a str>,
    /// What the footer offers for a job. A host with nothing to offer returns
    /// `[None, None]`.
    pub actions: fn(&Job) -> ActionSet,
    /// The tasks a host can start. Empty hides the header's `Run task…` pill,
    /// so a board with no runner offers no button that would do nothing.
    pub launches: &'a [Launch],
}

/// See the [module docs](self).
pub struct Board {
    sheet: SheetState,
    /// The sheet's own content scroll, when a long verify plan or fact set is
    /// taller than the card. The list behind holds still while it moves.
    sheet_scroll: Scroll,
    scroll: Scroll,
    /// By task rather than index: a rescan can reorder the list under the
    /// sheet, and the same job should stay open.
    selected: Option<String>,
    /// The task picker is open instead of a job's sheet. One or the other,
    /// never both.
    launcher: bool,
}

impl Default for Board {
    fn default() -> Self {
        Self::new()
    }
}

impl Board {
    pub fn new() -> Self {
        Self {
            sheet: SheetState::new(),
            sheet_scroll: Scroll::new(),
            scroll: Scroll::new(),
            selected: None,
            launcher: false,
        }
    }

    /// The board a review capture starts on with the task picker open. The
    /// picker reads the host's list, not the jobs, so it needs none.
    pub fn opened_on_launcher() -> Self {
        let mut board = Self::new();
        board.launcher = true;
        board.sheet = SheetState::new().opened();
        board
    }

    /// The board a review capture starts on: the first job in `phase`, its
    /// sheet already all the way up.
    pub fn opened_on(jobs: &[Job], phase: Phase) -> Self {
        match jobs.iter().find(|job| job.phase == phase) {
            Some(job) => Self::opened_on_task(jobs, &job.task),
            None => Self::new(),
        }
    }

    /// The board a review capture starts on, with `task`'s sheet already all
    /// the way up. A host that has a selected task (a key, a `--open` flag)
    /// starts here rather than animating a sheet nobody tapped.
    pub fn opened_on_task(jobs: &[Job], task: &str) -> Self {
        let mut board = Self::new();
        if let Some(job) = jobs.iter().find(|job| job.task == task) {
            board.selected = Some(job.task.clone());
            board.sheet = SheetState::new().opened();
        }
        board
    }

    /// The task whose sheet is open, if one is.
    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    pub fn draw(&mut self, p: &Painter, frame: BoardFrame) -> Option<BoardEvent> {
        let area = frame.area;
        let c = p.theme.color;
        draw_rectangle(area.x, area.y, area.w, area.h, c.bg_primary);
        self.sheet.update(frame.dt);

        // A job that has gone away cannot keep its sheet open.
        if !self.launcher
            && let Some(task) = &self.selected
            && !frame.jobs.iter().any(|job| &job.task == task)
        {
            self.selected = None;
            self.sheet.close();
        }

        if self.draw_header(p, &frame) {
            self.selected = None;
            self.launcher = true;
            self.sheet_scroll = Scroll::new();
            self.sheet.open();
        }

        let list = Rect::new(area.x, area.y + HEAD_H, area.w, (area.h - HEAD_H).max(0.0));
        if frame.jobs.is_empty() {
            p.text_in(
                "No jobs in this directory yet.",
                Rect::new(list.x + PAD, list.y + 12.0, list.w - PAD * 2.0, 20.0),
                p.size(TextSize::Md),
                c.text_muted,
                Align::Start,
            );
        }

        let cols: usize = if list.w >= TWO_COLUMNS_AT { 2 } else { 1 };
        let card_w = (list.w - PAD * 2.0 - GAP * (cols - 1) as f32) / cols as f32;
        let rows = frame.jobs.len().div_ceil(cols);
        let content_h = rows as f32 * CARD_H + rows.saturating_sub(1) as f32 * GAP + 8.0;

        // A sheet over the list holds the list's input back, but lets a coast
        // already under way run out.
        let behind_sheet = self.sheet.visible();
        let input = if behind_sheet {
            ScrollInput::none()
        } else {
            ScrollInput::read(frame.gesture.drag, Vec2::from(mouse_position()))
        };
        self.scroll
            .update(list, &input, Some((content_h - list.h).max(0.0)), frame.dt);
        // A tap that only caught a coasting list opens nothing.
        let tap = self.scroll.filter_tap(frame.gesture.tap);
        let filtered = Painter::new(p.font, p.mono, p.theme, tap).at_time(p.time());
        let behind = filtered.untapped();
        let cards_p = if behind_sheet { &behind } else { &filtered };

        let mut picked = None;
        with_clip(list, || {
            let off = self.scroll.offset();
            for (i, job) in frame.jobs.iter().enumerate() {
                let rect = Rect::new(
                    list.x + PAD + (i % cols) as f32 * (card_w + GAP),
                    list.y + 4.0 + (i / cols) as f32 * (CARD_H + GAP) - off,
                    card_w,
                    CARD_H,
                );
                if rect.bottom() < list.y || rect.y > list.bottom() {
                    continue;
                }
                let open = self.selected.as_deref() == Some(job.task.as_str());
                if self.card(cards_p, rect, job, open, frame.now_s, !behind_sheet) {
                    picked = Some(i);
                }
            }
        });
        if let Some(i) = picked {
            self.selected = Some(frame.jobs[i].task.clone());
            self.sheet_scroll = Scroll::new();
            self.sheet.open();
            return Some(BoardEvent::Opened {
                task: frame.jobs[i].task.clone(),
            });
        }

        if self.launcher {
            self.draw_launcher(p, area, &frame)
        } else if self.selected.is_some() {
            self.draw_sheet(p, area, &frame)
        } else {
            None
        }
    }

    /// The title band. Returns whether the host's `Run task…` pill was tapped.
    fn draw_header(&self, p: &Painter, frame: &BoardFrame) -> bool {
        let c = p.theme.color;
        let area = frame.area;
        let x = area.x + PAD;
        let w = (area.w - PAD * 2.0).max(0.0);
        let mut by_phase = [0usize; 4];
        for job in frame.jobs {
            let i = match job.phase {
                Phase::Running => 0,
                Phase::Verifying => 1,
                Phase::Done => 2,
                Phase::Failed => 3,
            };
            by_phase[i] += 1;
        }
        let active = by_phase[0] + by_phase[1];
        let repos: BTreeSet<&str> = frame
            .jobs
            .iter()
            .map(|job| job.repo.as_str())
            .filter(|repo| !repo.is_empty())
            .collect();
        let scope = match repos.len() {
            0 => String::new(),
            1 => repos.into_iter().next().unwrap_or_default().to_owned(),
            n => format!("{n} repos"),
        };
        let summary = format!(
            "{} job{} · {active} active · {} done · {} failed{}",
            frame.jobs.len(),
            if frame.jobs.len() == 1 { "" } else { "s" },
            by_phase[2],
            by_phase[3],
            if frame.jobs.iter().any(|job| job.usage.is_some()) {
                let spend: f64 = frame
                    .jobs
                    .iter()
                    .filter_map(|job| job.usage)
                    .map(|usage| usage.cost_usd)
                    .sum();
                format!(" · {}", cost_label(spend))
            } else {
                String::new()
            }
        );

        p.text_in(
            "JOBS",
            Rect::new(x, area.y + 10.0, w, 24.0),
            p.size(TextSize::Xl),
            c.accent,
            Align::Start,
        );
        // The pill takes the title row's end and the scope fits left of it, so
        // the band is the same height with or without a runner configured.
        let mut start = false;
        let pill_w = if frame.launches.is_empty() {
            0.0
        } else {
            let label = "Run a task";
            let width = p.measure(label, p.size(TextSize::Lg)).width + 40.0;
            let rect = Rect::new(area.right() - PAD - width, area.y + 7.0, width, 30.0);
            // Drawn behind a sheet's scrim, where it takes no tap.
            start = Pill::filled(label)
                .live(!self.sheet.visible())
                .show(p, rect);
            width + 10.0
        };
        if !scope.is_empty() {
            let scope_w = (w - pill_w).max(0.0);
            p.text_in(
                &p.fit(&scope, p.size(TextSize::Sm), scope_w * 0.6),
                Rect::new(x, area.y + 10.0, scope_w, 24.0),
                p.size(TextSize::Sm),
                c.text_muted,
                Align::End,
            );
        }
        p.text_in(
            &p.fit(&summary, p.size(TextSize::Xs), w),
            Rect::new(x, area.y + 36.0, w, 16.0),
            p.size(TextSize::Xs),
            c.text_secondary,
            Align::Start,
        );
        if let Some(status) = frame.status {
            p.text_in(
                &p.fit(status, p.size(TextSize::Xs), w),
                Rect::new(x, area.y + 54.0, w, 16.0),
                p.size(TextSize::Xs),
                c.accent,
                Align::Start,
            );
        }
        start
    }

    /// One card, and whether its body was tapped. `enabled` is false while the
    /// sheet is up, which holds the card's own taps and hover back.
    fn card(
        &self,
        p: &Painter,
        rect: Rect,
        job: &Job,
        open: bool,
        now_s: i64,
        enabled: bool,
    ) -> bool {
        let c = p.theme.color;
        surface(p, rect);
        let hit = p.interact(rect, enabled);
        if hit.hover {
            fill_rounded(rect, SURFACE_R, with_alpha(c.bg_highlight, 0.22));
        }
        if open {
            edge(rect, SURFACE_R, c.accent);
        }

        let mark = Vec2::new(rect.x + PAD + 7.0, rect.y + 22.0);
        phase_mark(p, mark, job.phase);

        // The phase word claims the end of the row first; the task takes what
        // is left and is shortened there rather than run under it.
        let word = job.phase.label();
        let word_w = p.measure(word, p.size(TextSize::Sm)).width + 4.0;
        p.text_in(
            word,
            Rect::new(rect.right() - PAD - word_w, rect.y + 10.0, word_w, 24.0),
            p.size(TextSize::Sm),
            phase_ink(job.phase, p),
            Align::End,
        );
        let name_x = rect.x + PAD + 20.0;
        let name_w = (rect.right() - PAD - word_w - 10.0 - name_x).max(0.0);
        p.text_in(
            &p.fit(job.short_task(), p.size(TextSize::Lg), name_w),
            Rect::new(name_x, rect.y + 10.0, name_w, 24.0),
            p.size(TextSize::Lg),
            c.text_primary,
            Align::Start,
        );

        let base = base_short(&job.base);
        let base_w = if base.is_empty() {
            0.0
        } else {
            p.measure_mono(&base, p.size(TextSize::Sm)).width + 10.0
        };
        if !job.repo.is_empty() {
            let repo_w = (rect.w - PAD * 2.0 - base_w).max(0.0);
            p.text_in(
                &p.fit(&job.repo, p.size(TextSize::Sm), repo_w),
                Rect::new(rect.x + PAD, rect.y + 38.0, repo_w, 16.0),
                p.size(TextSize::Sm),
                c.text_secondary,
                Align::Start,
            );
        }
        if !base.is_empty() {
            p.mono_in(
                &base,
                Rect::new(rect.x + PAD, rect.y + 38.0, rect.w - PAD * 2.0, 16.0),
                p.size(TextSize::Sm),
                c.text_muted,
                Align::End,
            );
        }

        let pips = verify_pips(p, Vec2::new(rect.x + PAD, rect.y + 68.0), job);
        let (summary, ink) = verify_count(p, job);
        p.text_in(
            &summary,
            Rect::new(rect.x + PAD + pips + 8.0, rect.y + 60.0, 160.0, 16.0),
            p.size(TextSize::Xs),
            ink,
            Align::Start,
        );
        let spend = job
            .usage
            .map(|usage| format!(" · {}", cost_label(usage.cost_usd)))
            .unwrap_or_default();
        p.mono_in(
            &format!("{}{spend}", elapsed_label(job, now_s)),
            Rect::new(rect.right() - PAD - 160.0, rect.y + 60.0, 160.0, 16.0),
            p.size(TextSize::Sm),
            c.text_secondary,
            Align::End,
        );
        hit.clicked
    }

    fn draw_sheet(&mut self, p: &Painter, area: Rect, frame: &BoardFrame) -> Option<BoardEvent> {
        let task = self.selected.as_deref()?;
        let job = frame.jobs.iter().find(|job| job.task == task)?.clone();
        let base = base_short(&job.base);
        let subtitle = [job.repo.as_str(), job.phase.label(), base.as_str()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        let sheet = Sheet::new()
            .title(job.short_task())
            .subtitle(&subtitle)
            .tint(phase_ink(job.phase, p));

        let mut verify_rows = Vec::new();
        let (summary, summary_ink) = verify_summary(p, &job);
        verify_rows.push(
            WellRow::inline("verify", &summary)
                .ink(summary_ink)
                .strong(true),
        );
        for step in &job.verify {
            // A read job's label is its command; the small label fits one
            // whole where the value's size would cut it.
            verify_rows.push(
                WellRow::stacked(&step.label, step.state.label()).ink(step_ink(step.state, p)),
            );
        }

        let mut facts = vec![WellRow::stacked("task", &job.task)];
        if let Some(session) = &job.session {
            facts.push(WellRow::stacked("session", session).mono());
        }
        if !base.is_empty() {
            facts.push(WellRow::inline("base", &base).mono());
        }
        if let Some(image) = &job.image {
            facts.push(WellRow::stacked("agent image", &image.reference).mono());
        }
        if let Some(image) = &job.verify_image {
            facts.push(WellRow::stacked("verify image", &image.reference).mono());
        }
        let digests = match (&job.image, &job.verify_image) {
            (Some(image), Some(verify_image)) => Some(format!(
                "{} · {}",
                digest_label(&image.digest),
                digest_label(&verify_image.digest)
            )),
            _ => None,
        };
        if let Some(digests) = &digests {
            facts.push(WellRow::inline("digests", digests).mono());
        }
        if let Some(model) = &job.model {
            facts.push(WellRow::inline("model", model).mono());
        }
        let cost = job.usage.map(|usage| cost_label(usage.cost_usd));
        if let Some(cost) = &cost {
            facts.push(WellRow::inline("cost", cost));
        }
        let tokens = job
            .usage
            .filter(|usage| {
                usage.model_calls > 0 || usage.input_tokens > 0 || usage.output_tokens > 0
            })
            .map(tokens_label);
        if let Some(tokens) = &tokens {
            facts.push(WellRow::inline("tokens", tokens).mono());
        }
        let cached_usage = job
            .usage
            .filter(|usage| usage.cached_input_tokens > 0 && usage.input_tokens > 0);
        let cached = cached_usage.map(cached_label);
        if let (Some(cached), Some(usage)) = (&cached, cached_usage) {
            facts.push(WellRow::inline("cached", cached).mono().meter(
                usage.cached_input_tokens as f32 / usage.input_tokens as f32,
                p.theme.color.success,
            ));
        }
        let patch = job.patch.map(patch_label);
        if let Some(patch) = &patch {
            facts.push(WellRow::inline("patch", patch).mono());
        }
        let effects = job.effects.map(effects_label);
        if let Some(effects) = &effects {
            facts.push(WellRow::inline("effects", effects));
        }

        let verify_h = well_height(p, &verify_rows);
        let facts_h = well_height(p, &facts);
        let content_h = verify_h + DETAIL_GAP + facts_h;
        let form = macroquad_widgets::SheetForm::for_area(area);
        let height = sheet_chrome(form, area, &sheet) + content_h;
        let card = sheet_card(p, &self.sheet, area, height, &sheet)?;
        if card.dismissed {
            self.sheet.close();
            return None;
        }

        // A plan longer than the card scrolls above a footer that stays put;
        // the list behind takes no input while a sheet is up.
        let input = if card.live {
            ScrollInput::read(frame.gesture.drag, Vec2::from(mouse_position()))
        } else {
            ScrollInput::none()
        };
        self.sheet_scroll.update(
            card.content,
            &input,
            Some((content_h - card.content.h).max(0.0)),
            frame.dt,
        );
        let off = self.sheet_scroll.offset();
        with_clip(card.content, || {
            let verify_rect = Rect::new(
                card.content.x,
                card.content.y - off,
                card.content.w,
                verify_h,
            );
            well(p, verify_rect, &verify_rows, card.live, card.alpha);
            let facts_rect = Rect::new(
                card.content.x,
                verify_rect.bottom() + DETAIL_GAP,
                card.content.w,
                facts_h,
            );
            well(p, facts_rect, &facts, card.live, card.alpha);
        });

        let actions = (frame.actions)(&job);
        let present: Vec<(usize, Action)> = actions
            .iter()
            .enumerate()
            .filter_map(|(i, action)| action.map(|action| (i, action)))
            .collect();
        if present.is_empty() {
            return None;
        }
        let rects = pill_row(card.footer, present.len(), 10.0, Some(0.45));
        for ((index, action), rect) in present.into_iter().zip(rects) {
            let pill = match action.style {
                PillStyle::Filled => Pill::filled(action.label),
                PillStyle::Ghost => Pill::ghost(action.label),
                PillStyle::Danger => Pill::danger(action.label),
            };
            if pill.live(card.live).alpha(card.alpha).show(p, rect) {
                return Some(BoardEvent::Action {
                    task: job.task.clone(),
                    index,
                });
            }
        }
        None
    }

    /// The task picker: one row per task the host may start, each with its own
    /// `Run` pill. A row is a label, not a hit area — the pill is the action, as
    /// every action in this board is.
    fn draw_launcher(&mut self, p: &Painter, area: Rect, frame: &BoardFrame) -> Option<BoardEvent> {
        const ROW_H: f32 = 52.0;
        const ROW_GAP: f32 = 8.0;
        const PILL_W: f32 = 72.0;
        const PILL_H: f32 = 30.0;

        let sheet = Sheet::new()
            .title("New run")
            .subtitle("starts one task in a container")
            .footer(false);
        let rows = frame.launches.len();
        let content_h = if rows == 0 {
            22.0
        } else {
            rows as f32 * ROW_H + (rows - 1) as f32 * ROW_GAP
        };
        let form = SheetForm::for_area(area);
        let height = sheet_chrome(form, area, &sheet) + content_h;
        let card = sheet_card(p, &self.sheet, area, height, &sheet)?;
        if card.dismissed {
            self.sheet.close();
            self.launcher = false;
            return None;
        }

        let input = if card.live {
            ScrollInput::read(frame.gesture.drag, Vec2::from(mouse_position()))
        } else {
            ScrollInput::none()
        };
        self.sheet_scroll.update(
            card.content,
            &input,
            Some((content_h - card.content.h).max(0.0)),
            frame.dt,
        );
        let off = self.sheet_scroll.offset();

        if frame.launches.is_empty() {
            p.text_in(
                "No task files are configured.",
                Rect::new(card.content.x, card.content.y, card.content.w, 20.0),
                p.size(TextSize::Md),
                p.theme.color.text_muted,
                Align::Start,
            );
            return None;
        }

        let mut picked = None;
        with_clip(card.content, || {
            for (index, launch) in frame.launches.iter().enumerate() {
                let rect = Rect::new(
                    card.content.x,
                    card.content.y - off + index as f32 * (ROW_H + ROW_GAP),
                    card.content.w,
                    ROW_H,
                );
                if rect.bottom() < card.content.y || rect.y > card.content.bottom() {
                    continue;
                }
                surface(p, rect);
                let pill = Rect::new(
                    rect.right() - PAD - PILL_W,
                    rect.center().y - PILL_H * 0.5,
                    PILL_W,
                    PILL_H,
                );
                let name_w = (pill.x - 10.0 - (rect.x + PAD)).max(0.0);
                p.text_in(
                    &p.fit(&launch.name, p.size(TextSize::Lg), name_w),
                    Rect::new(rect.x + PAD, rect.y, name_w, ROW_H),
                    p.size(TextSize::Lg),
                    p.theme.color.text_primary,
                    Align::Start,
                );
                if Pill::filled("Run")
                    .live(card.live)
                    .alpha(card.alpha)
                    .show(p, pill)
                {
                    picked = Some(index);
                }
            }
        });

        if let Some(index) = picked {
            self.launcher = false;
            self.sheet.close();
            return Some(BoardEvent::Launch { index });
        }
        None
    }
}

fn phase_ink(phase: Phase, p: &Painter) -> Color {
    let c = p.theme.color;
    match phase {
        Phase::Running => c.accent,
        Phase::Verifying => c.warning,
        Phase::Done => c.success,
        Phase::Failed => c.error,
    }
}

fn step_ink(state: StepState, p: &Painter) -> Color {
    let c = p.theme.color;
    match state {
        StepState::Pending => c.text_muted,
        StepState::Ok => c.success,
        StepState::Failed => c.error,
    }
}

/// The phase's mark: a turning ring while it is working, a check when it
/// landed, a cross when it did not.
fn phase_mark(p: &Painter, at: Vec2, phase: Phase) {
    let c = p.theme.color;
    match phase {
        Phase::Running | Phase::Verifying => {
            Spinner::busy()
                .ink(phase_ink(phase, p))
                .show(p, Rect::new(at.x - 7.0, at.y - 7.0, 14.0, 14.0));
        }
        Phase::Done => {
            macroquad_widgets::check_badge(at, 5.5, c.success, c.bg_secondary, 1.0);
        }
        Phase::Failed => {
            draw_circle(at.x, at.y, 5.5, c.error);
            draw_line(
                at.x - 2.3,
                at.y - 2.3,
                at.x + 2.3,
                at.y + 2.3,
                1.6,
                c.bg_secondary,
            );
            draw_line(
                at.x - 2.3,
                at.y + 2.3,
                at.x + 2.3,
                at.y - 2.3,
                1.6,
                c.bg_secondary,
            );
        }
    }
}

/// One pip per verify command, in the ink its outcome carries. Returns the
/// row's width, so the count can start after it.
fn verify_pips(p: &Painter, at: Vec2, job: &Job) -> f32 {
    const PIP_W: f32 = 18.0;
    const PIP_H: f32 = 4.0;
    const PIP_GAP: f32 = 4.0;
    if job.verify.is_empty() {
        return 0.0;
    }
    let c = p.theme.color;
    for (i, step) in job.verify.iter().enumerate() {
        let ink = match step.state {
            StepState::Pending => c.bg_highlight,
            StepState::Ok => c.success,
            StepState::Failed => c.error,
        };
        fill_rounded(
            Rect::new(at.x + i as f32 * (PIP_W + PIP_GAP), at.y, PIP_W, PIP_H),
            PIP_H * 0.5,
            ink,
        );
    }
    job.verify.len() as f32 * (PIP_W + PIP_GAP) - PIP_GAP
}

/// The count on a card and the ink it should carry: green only when every
/// command has passed, red as soon as one has failed, muted while any is still
/// waiting.
fn verify_count(p: &Painter, job: &Job) -> (String, Color) {
    let c = p.theme.color;
    if job.verify.is_empty() {
        return ("no verify".to_string(), c.text_muted);
    }
    let ink = if job.verify.iter().any(|s| s.state == StepState::Failed) {
        c.error
    } else if job.passed() == job.verify.len() {
        c.success
    } else {
        c.text_muted
    };
    (format!("{}/{}", job.passed(), job.verify.len()), ink)
}

/// The sentence a sheet's verify panel leads with.
fn verify_summary(p: &Painter, job: &Job) -> (String, Color) {
    let c = p.theme.color;
    let total = job.verify.len();
    let (_, ink) = verify_count(p, job);
    if total == 0 {
        return ("not run".to_string(), c.text_muted);
    }
    if job.phase == Phase::Running
        && job
            .verify
            .iter()
            .all(|step| step.state == StepState::Pending)
    {
        return ("waiting on the agent".to_string(), c.text_muted);
    }
    (format!("{} of {total} passed", job.passed()), ink)
}

fn elapsed_label(job: &Job, now_s: i64) -> String {
    match job.elapsed_s(now_s) {
        Some(seconds) => duration(seconds),
        None => "--".to_string(),
    }
}

fn duration(seconds: i64) -> String {
    let s = seconds.max(0);
    if s < 60 {
        format!("{s}s")
    } else if s < 3_600 {
        format!("{}m {:02}s", s / 60, s % 60)
    } else {
        format!("{}h {:02}m", s / 3_600, (s % 3_600) / 60)
    }
}

fn base_short(base: &str) -> String {
    base.chars().take(7).collect()
}

fn digest_label(digest: &str) -> String {
    match digest.strip_prefix("sha256:") {
        // The row is labelled "digests", so the shared algorithm is not
        // repeated at both ends of it.
        Some(hex) => hex.chars().take(7).collect(),
        None => digest.chars().take(14).collect(),
    }
}

fn cost_label(cost: f64) -> String {
    if cost < 0.01 {
        format!("${cost:.4}")
    } else {
        format!("${cost:.2}")
    }
}

fn tokens_label(usage: Usage) -> String {
    let calls = match usage.model_calls {
        0 => String::new(),
        1 => "1 call · ".to_string(),
        calls => format!("{calls} calls · "),
    };
    format!(
        "{calls}{} in · {} out",
        grouped(usage.input_tokens),
        grouped(usage.output_tokens)
    )
}

fn cached_label(usage: Usage) -> String {
    format!(
        "{} of {} in",
        grouped(usage.cached_input_tokens),
        grouped(usage.input_tokens)
    )
}

fn patch_label(patch: jobs::Patch) -> String {
    let applies = if patch.applies_on_host {
        "applies on host"
    } else {
        "does not apply"
    };
    format!("{} B · {applies}", grouped(patch.bytes))
}

fn effects_label(effects: jobs::Effects) -> String {
    format!("{} events · {}", effects.events, bytes_label(effects.bytes))
}

fn bytes_label(bytes: u64) -> String {
    if bytes < 1_024 {
        format!("{bytes} B")
    } else if bytes < 1_024 * 1_024 {
        format!("{} KB", bytes / 1_024)
    } else {
        format!("{:.1} MB", bytes as f64 / (1_024.0 * 1_024.0))
    }
}

/// `1765` as `1,765`.
fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_duration_reads_in_the_largest_unit_that_fits() {
        assert_eq!(duration(0), "0s");
        assert_eq!(duration(83), "1m 23s");
        assert_eq!(duration(3_600), "1h 00m");
        assert_eq!(duration(7_320), "2h 02m");
    }

    #[test]
    fn bytes_stay_readable() {
        assert_eq!(bytes_label(9), "9 B");
        assert_eq!(bytes_label(1_024), "1 KB");
        assert_eq!(bytes_label(137_216), "134 KB");
        assert_eq!(bytes_label(2 * 1_024 * 1_024), "2.0 MB");
    }

    #[test]
    fn a_patch_is_grouped_and_says_whether_it_applies() {
        assert_eq!(
            patch_label(jobs::Patch {
                bytes: 1_765,
                applies_on_host: true
            }),
            "1,765 B · applies on host"
        );
    }

    #[test]
    fn a_cost_keeps_the_precision_small_runs_need() {
        assert_eq!(cost_label(0.0022), "$0.0022");
        assert_eq!(cost_label(1.234), "$1.23");
    }

    #[test]
    fn usage_reads_as_calls_and_tokens() {
        let usage = Usage {
            model_calls: 3,
            input_tokens: 12_400,
            output_tokens: 800,
            cached_input_tokens: 9_000,
            cost_usd: 0.0022,
        };
        assert_eq!(tokens_label(usage), "3 calls · 12,400 in · 800 out");
        assert_eq!(cached_label(usage), "9,000 of 12,400 in");
    }

    #[test]
    fn a_digest_is_shortened_and_the_algorithm_is_the_rows_label() {
        assert_eq!(
            digest_label("sha256:ba4eb717a58edace149255bfc0338923"),
            "ba4eb71"
        );
        assert_eq!(digest_label("md5:abcdef"), "md5:abcdef");
    }
}
