//! Live-control pacing for the confirmed Candidate A watched-match simulation.
//!
//! Stage 3 proved Candidate A can be paced at ~60 ticks/s and irreversibly released with Ctrl+End.
//! Stage 4 proved manual `InputV1::move_to` commands reach that live simulation.
//!
//! Stage 5A's zero-tick prematch hold was physically rejected: blocking the very first AI callback
//! prevents Start Match from completing, and the render thread stops pumping while it waits. This
//! revision allows one complete Candidate-A simulation tick through before holding on the next tick.
//! That tests whether the client only needs an initial simulation frame/state to construct InGame.
//!
//! The held Candidate-A worker now also polls Ctrl+Home directly. That escape does not depend on
//! `post_render`, so it still works if the Start Match UI thread is synchronously waiting. If one
//! tick is insufficient and the client has not reached InGame after two seconds, the gate
//! automatically releases into the known-good 60 Hz pacer rather than leaving the process hung.
//!
//! Pause uses a separate presentation gate. Ctrl+End permanently releases pacing and manual input
//! for the current match.

use std::{
    sync::atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering},
    thread,
    time::Duration,
};

use mod_api_stable::{InputV1, StableAiContext, StableAiInit, StablePlayerAi};
use windows_sys::Win32::{
    System::Threading::GetCurrentThreadId,
    UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_HOME},
};

use crate::{control, simulation_probe};

const NO_TICK: u64 = u64::MAX;
const NO_PLAYER: usize = usize::MAX;
const ALLOWED_LEAD_MS: u64 = 35;
const MAX_SLEEP_SLICE_MS: u64 = 2;
const MAX_SINGLE_CALLBACK_WAIT_MS: u64 = 250;
const RENDER_HEARTBEAT_STALE_MS: u64 = 500;
const BLOCK_SLEEP_SLICE_MS: u64 = 2;
const PREMATCH_AUTO_RELEASE_MS: u64 = 2_000;

const PHASE_WAITING_START: u8 = 0;
const PHASE_RUNNING: u8 = 1;
const PHASE_PAUSED: u8 = 2;

#[link(name = "kernel32")]
extern "system" {
    fn GetTickCount64() -> u64;
}

static TOTAL_THINK_CALLS: AtomicU64 = AtomicU64::new(0);
static CANDIDATE_A_THINK_CALLS: AtomicU64 = AtomicU64::new(0);
static FIRST_CANDIDATE_A_TICK: AtomicU64 = AtomicU64::new(NO_TICK);
static LAST_CANDIDATE_A_TICK: AtomicU64 = AtomicU64::new(NO_TICK);
static LAST_CANDIDATE_A_PLAYER: AtomicUsize = AtomicUsize::new(NO_PLAYER);
static LAST_CANDIDATE_A_ATHLETE: AtomicUsize = AtomicUsize::new(NO_PLAYER);
static LAST_CANDIDATE_A_THREAD: AtomicU64 = AtomicU64::new(0);
static SEEN_PLAYER_MASK: AtomicU64 = AtomicU64::new(0);

// Candidate-A's detoured wrapper gives us a stable job identity before StablePlayerAi executes.
// Reset the per-match latches at that boundary rather than at the later InGame scene transition.
static ACTIVE_JOB_CONTEXT: AtomicUsize = AtomicUsize::new(0);
static ACTIVE_JOB_ENTRY: AtomicU64 = AtomicU64::new(0);

static START_REQUESTED: AtomicBool = AtomicBool::new(false);
static START_AUTO_RELEASED: AtomicBool = AtomicBool::new(false);
static INTERACTIVE_MATCH: AtomicBool = AtomicBool::new(false);
static LAST_RENDER_HEARTBEAT_MS: AtomicU64 = AtomicU64::new(0);
static PRESENTATION_PHASE: AtomicU8 = AtomicU8::new(PHASE_WAITING_START);
static PACER_ORIGIN_TICK: AtomicU64 = AtomicU64::new(NO_TICK);
static PACER_ORIGIN_MS: AtomicU64 = AtomicU64::new(0);
static MANUAL_FINISH_REQUESTED: AtomicBool = AtomicBool::new(false);
static SAFETY_FAIL_OPEN: AtomicBool = AtomicBool::new(false);
static PACER_WAIT_COUNT: AtomicU64 = AtomicU64::new(0);
static PACER_TOTAL_WAIT_MS: AtomicU64 = AtomicU64::new(0);
static START_WAIT_COUNT: AtomicU64 = AtomicU64::new(0);
static START_TOTAL_WAIT_MS: AtomicU64 = AtomicU64::new(0);
static PAUSE_WAIT_COUNT: AtomicU64 = AtomicU64::new(0);
static PAUSE_TOTAL_WAIT_MS: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy)]
pub struct PacingProbeSnapshot {
    pub total_think_calls: u64,
    pub candidate_a_think_calls: u64,
    pub first_candidate_a_tick: Option<u64>,
    pub last_candidate_a_tick: Option<u64>,
    pub last_candidate_a_player: Option<usize>,
    pub last_candidate_a_athlete: Option<usize>,
    pub last_candidate_a_thread: u32,
    pub seen_player_mask: u64,
    pub pacer_origin_tick: Option<u64>,
    pub pacer_elapsed_ms: u64,
    pub start_requested: bool,
    pub start_auto_released: bool,
    pub manual_finish_requested: bool,
    pub safety_fail_open: bool,
    pub pacer_wait_count: u64,
    pub pacer_total_wait_ms: u64,
    pub start_wait_count: u64,
    pub start_total_wait_ms: u64,
    pub pause_wait_count: u64,
    pub pause_total_wait_ms: u64,
    pub presentation_phase: u8,
    pub interactive_match: bool,
    pub active_job_context: usize,
    pub active_job_entry: u64,
}

#[derive(Debug, Clone, Default)]
pub struct CandidateAObserverAi;

fn reanchor_pacer() {
    PACER_ORIGIN_TICK.store(NO_TICK, Ordering::Release);
    PACER_ORIGIN_MS.store(0, Ordering::Release);
}

fn reset_job_runtime() {
    CANDIDATE_A_THINK_CALLS.store(0, Ordering::Release);
    FIRST_CANDIDATE_A_TICK.store(NO_TICK, Ordering::Release);
    LAST_CANDIDATE_A_TICK.store(NO_TICK, Ordering::Release);
    LAST_CANDIDATE_A_PLAYER.store(NO_PLAYER, Ordering::Release);
    LAST_CANDIDATE_A_ATHLETE.store(NO_PLAYER, Ordering::Release);
    LAST_CANDIDATE_A_THREAD.store(0, Ordering::Release);
    SEEN_PLAYER_MASK.store(0, Ordering::Release);

    START_REQUESTED.store(false, Ordering::Release);
    START_AUTO_RELEASED.store(false, Ordering::Release);
    LAST_RENDER_HEARTBEAT_MS.store(0, Ordering::Release);
    PRESENTATION_PHASE.store(PHASE_WAITING_START, Ordering::Release);
    reanchor_pacer();
    MANUAL_FINISH_REQUESTED.store(false, Ordering::Release);
    SAFETY_FAIL_OPEN.store(false, Ordering::Release);
    PACER_WAIT_COUNT.store(0, Ordering::Release);
    PACER_TOTAL_WAIT_MS.store(0, Ordering::Release);
    START_WAIT_COUNT.store(0, Ordering::Release);
    START_TOTAL_WAIT_MS.store(0, Ordering::Release);
    PAUSE_WAIT_COUNT.store(0, Ordering::Release);
    PAUSE_TOTAL_WAIT_MS.store(0, Ordering::Release);
}

/// Called when the client leaves an interactive match. The actual Candidate-A job boundary is also
/// detected on the simulation thread; this prevents stale controls surviving a long menu interval.
pub fn prepare_next_match() {
    INTERACTIVE_MATCH.store(false, Ordering::Release);
    ACTIVE_JOB_CONTEXT.store(0, Ordering::Release);
    ACTIVE_JOB_ENTRY.store(0, Ordering::Release);
    reset_job_runtime();
}

fn observe_candidate_job(probe: simulation_probe::SimulationProbeSnapshot) {
    let previous_context = ACTIVE_JOB_CONTEXT.load(Ordering::Acquire);
    let previous_entry = ACTIVE_JOB_ENTRY.load(Ordering::Acquire);
    if previous_context == probe.last_context && previous_entry == probe.entries {
        return;
    }

    ACTIVE_JOB_CONTEXT.store(probe.last_context, Ordering::Release);
    ACTIVE_JOB_ENTRY.store(probe.entries, Ordering::Release);
    reset_job_runtime();
}

/// Starts the held Candidate-A simulation and re-anchors the 60 Hz wall-clock pacer.
pub fn request_start_simulation() {
    if manual_control_released() {
        return;
    }

    START_REQUESTED.store(true, Ordering::Release);
    PRESENTATION_PHASE.store(PHASE_RUNNING, Ordering::Release);
    reanchor_pacer();
}

/// Publish a render heartbeat independently of pause-state inference. If the client stops
/// rendering while a live match session still exists, Candidate A will fail closed and wait
/// rather than silently simulating ahead with vanilla AI.
pub fn note_render_heartbeat() {
    LAST_RENDER_HEARTBEAT_MS.store(unsafe { GetTickCount64() }, Ordering::Release);
}

/// Publish interactive/pause state from the client render thread.
pub fn set_presentation_state(interactive_match: bool, paused: bool) {
    INTERACTIVE_MATCH.store(interactive_match, Ordering::Release);

    if !START_REQUESTED.load(Ordering::Acquire) {
        PRESENTATION_PHASE.store(PHASE_WAITING_START, Ordering::Release);
        return;
    }

    let next = if interactive_match && paused {
        PHASE_PAUSED
    } else {
        PHASE_RUNNING
    };

    let previous = PRESENTATION_PHASE.swap(next, Ordering::AcqRel);
    if next == PHASE_RUNNING && previous == PHASE_PAUSED {
        // Paused wall time must never become runnable catch-up budget.
        reanchor_pacer();
    }
}

pub fn presentation_phase_label() -> &'static str {
    if manual_control_released() {
        return "RELEASED";
    }

    match PRESENTATION_PHASE.load(Ordering::Acquire) {
        PHASE_WAITING_START => {
            if INTERACTIVE_MATCH.load(Ordering::Acquire) {
                "READY / WAITING CTRL+HOME"
            } else {
                "HOLDING AFTER 1 STARTUP TICK"
            }
        }
        PHASE_PAUSED => "PAUSED",
        PHASE_RUNNING => {
            if INTERACTIVE_MATCH.load(Ordering::Acquire) {
                "RUNNING"
            } else if START_AUTO_RELEASED.load(Ordering::Acquire) {
                "PREMATCH / AUTO-RELEASED TO 60HZ"
            } else {
                "PREMATCH / RUNNING"
            }
        }
        _ => "UNKNOWN",
    }
}

pub fn start_requested() -> bool {
    START_REQUESTED.load(Ordering::Acquire)
}

pub fn presentation_running() -> bool {
    PRESENTATION_PHASE.load(Ordering::Acquire) == PHASE_RUNNING
}

/// Permanently releases direct control and pacing for the current match.
pub fn request_finish_simulation() {
    MANUAL_FINISH_REQUESTED.store(true, Ordering::Release);
}

pub fn manual_control_released() -> bool {
    // Only an explicit global release may permanently relinquish live pacing/control.
    // A transient pacer anomaly is recoverable and must never silently become Ctrl+End.
    MANUAL_FINISH_REQUESTED.load(Ordering::Acquire)
}

pub fn manual_input_enabled() -> bool {
    START_REQUESTED.load(Ordering::Acquire)
        && INTERACTIVE_MATCH.load(Ordering::Acquire)
        && presentation_running()
        && !manual_control_released()
}

pub fn snapshot() -> PacingProbeSnapshot {
    let first_tick = FIRST_CANDIDATE_A_TICK.load(Ordering::Acquire);
    let last_tick = LAST_CANDIDATE_A_TICK.load(Ordering::Acquire);
    let last_player = LAST_CANDIDATE_A_PLAYER.load(Ordering::Acquire);
    let last_athlete = LAST_CANDIDATE_A_ATHLETE.load(Ordering::Acquire);
    let pacer_origin_tick = PACER_ORIGIN_TICK.load(Ordering::Acquire);
    let pacer_origin_ms = PACER_ORIGIN_MS.load(Ordering::Acquire);
    let now_ms = unsafe { GetTickCount64() };

    PacingProbeSnapshot {
        total_think_calls: TOTAL_THINK_CALLS.load(Ordering::Acquire),
        candidate_a_think_calls: CANDIDATE_A_THINK_CALLS.load(Ordering::Acquire),
        first_candidate_a_tick: (first_tick != NO_TICK).then_some(first_tick),
        last_candidate_a_tick: (last_tick != NO_TICK).then_some(last_tick),
        last_candidate_a_player: (last_player != NO_PLAYER).then_some(last_player),
        last_candidate_a_athlete: (last_athlete != NO_PLAYER).then_some(last_athlete),
        last_candidate_a_thread: LAST_CANDIDATE_A_THREAD.load(Ordering::Acquire) as u32,
        seen_player_mask: SEEN_PLAYER_MASK.load(Ordering::Acquire),
        pacer_origin_tick: (pacer_origin_tick != NO_TICK).then_some(pacer_origin_tick),
        pacer_elapsed_ms: if pacer_origin_ms == 0 {
            0
        } else {
            now_ms.saturating_sub(pacer_origin_ms)
        },
        start_requested: START_REQUESTED.load(Ordering::Acquire),
        start_auto_released: START_AUTO_RELEASED.load(Ordering::Acquire),
        manual_finish_requested: MANUAL_FINISH_REQUESTED.load(Ordering::Acquire),
        safety_fail_open: SAFETY_FAIL_OPEN.load(Ordering::Acquire),
        pacer_wait_count: PACER_WAIT_COUNT.load(Ordering::Acquire),
        pacer_total_wait_ms: PACER_TOTAL_WAIT_MS.load(Ordering::Acquire),
        start_wait_count: START_WAIT_COUNT.load(Ordering::Acquire),
        start_total_wait_ms: START_TOTAL_WAIT_MS.load(Ordering::Acquire),
        pause_wait_count: PAUSE_WAIT_COUNT.load(Ordering::Acquire),
        pause_total_wait_ms: PAUSE_TOTAL_WAIT_MS.load(Ordering::Acquire),
        presentation_phase: PRESENTATION_PHASE.load(Ordering::Acquire),
        interactive_match: INTERACTIVE_MATCH.load(Ordering::Acquire),
        active_job_context: ACTIVE_JOB_CONTEXT.load(Ordering::Acquire),
        active_job_entry: ACTIVE_JOB_ENTRY.load(Ordering::Acquire),
    }
}

fn candidate_a_probe_for_thread(thread_id: u32) -> Option<simulation_probe::SimulationProbeSnapshot> {
    let candidate_a = simulation_probe::snapshots()[0];
    (candidate_a.active != 0
        && candidate_a.last_thread_id != 0
        && candidate_a.last_thread_id == thread_id)
        .then_some(candidate_a)
}

fn ctrl_home_down() -> bool {
    unsafe {
        GetAsyncKeyState(VK_CONTROL as i32) < 0 && GetAsyncKeyState(VK_HOME as i32) < 0
    }
}

fn wait_until_started() -> bool {
    let wait_started_ms = unsafe { GetTickCount64() };

    loop {
        if manual_control_released() {
            return false;
        }
        if START_REQUESTED.load(Ordering::Acquire) {
            return true;
        }

        // `post_render` is not guaranteed to run while Start Match waits. Poll the escape chord on
        // this worker too, so Ctrl+Home can always release a rejected prematch gate experiment.
        if ctrl_home_down() {
            request_start_simulation();
            return true;
        }

        // If the one-tick runway was insufficient, recover automatically before Windows decides
        // the process is hung. Once InGame is actually visible, do NOT auto-start: leave the held
        // tick waiting for the user's deliberate Ctrl+Home.
        let now_ms = unsafe { GetTickCount64() };
        if !INTERACTIVE_MATCH.load(Ordering::Acquire)
            && now_ms.saturating_sub(wait_started_ms) >= PREMATCH_AUTO_RELEASE_MS
        {
            START_AUTO_RELEASED.store(true, Ordering::Release);
            request_start_simulation();
            return true;
        }

        START_WAIT_COUNT.fetch_add(1, Ordering::Relaxed);
        START_TOTAL_WAIT_MS.fetch_add(BLOCK_SLEEP_SLICE_MS, Ordering::Relaxed);
        thread::sleep(Duration::from_millis(BLOCK_SLEEP_SLICE_MS));
    }
}

fn wait_while_paused() -> bool {
    loop {
        if manual_control_released() {
            return false;
        }
        if PRESENTATION_PHASE.load(Ordering::Acquire) != PHASE_PAUSED {
            return true;
        }

        PAUSE_WAIT_COUNT.fetch_add(1, Ordering::Relaxed);
        PAUSE_TOTAL_WAIT_MS.fetch_add(BLOCK_SLEEP_SLICE_MS, Ordering::Relaxed);
        thread::sleep(Duration::from_millis(BLOCK_SLEEP_SLICE_MS));
    }
}

fn render_heartbeat_stale() -> bool {
    if !INTERACTIVE_MATCH.load(Ordering::Acquire) {
        return false;
    }

    let heartbeat = LAST_RENDER_HEARTBEAT_MS.load(Ordering::Acquire);
    if heartbeat == 0 {
        return false;
    }

    unsafe { GetTickCount64() }.saturating_sub(heartbeat) >= RENDER_HEARTBEAT_STALE_MS
}

fn wait_while_render_stalled() -> bool {
    while render_heartbeat_stale() {
        if manual_control_released() {
            return false;
        }

        PAUSE_WAIT_COUNT.fetch_add(1, Ordering::Relaxed);
        PAUSE_TOTAL_WAIT_MS.fetch_add(BLOCK_SLEEP_SLICE_MS, Ordering::Relaxed);
        thread::sleep(Duration::from_millis(BLOCK_SLEEP_SLICE_MS));
    }
    true
}

fn pace_candidate_a(tick: u64) {
    if manual_control_released() {
        return;
    }

    if !START_REQUESTED.load(Ordering::Acquire) {
        let first_tick = FIRST_CANDIDATE_A_TICK.load(Ordering::Acquire);

        // Let every player callback belonging to the first observed simulation tick pass. The hold
        // begins only when Candidate A asks for input on a later tick, which proves the first tick
        // completed rather than freezing halfway through its ten players.
        if first_tick == NO_TICK || tick <= first_tick {
            return;
        }

        if !wait_until_started() {
            return;
        }
        // The exact held tick becomes the fresh pacing origin. Time spent waiting is never catch-up
        // budget, whether release came from the visible UI, worker-local Ctrl+Home, or auto-recovery.
        reanchor_pacer();
        return pace_candidate_a(tick);
    }

    if PRESENTATION_PHASE.load(Ordering::Acquire) == PHASE_PAUSED {
        if !wait_while_paused() {
            return;
        }
        reanchor_pacer();
        return pace_candidate_a(tick);
    }

    if render_heartbeat_stale() {
        if !wait_while_render_stalled() {
            return;
        }
        reanchor_pacer();
        return pace_candidate_a(tick);
    }

    let now_ms = unsafe { GetTickCount64() };
    let origin_tick = match PACER_ORIGIN_TICK.compare_exchange(
        NO_TICK,
        tick,
        Ordering::AcqRel,
        Ordering::Acquire,
    ) {
        Ok(_) => {
            PACER_ORIGIN_MS.store(now_ms, Ordering::Release);
            tick
        }
        Err(existing) => existing,
    };

    let origin_ms = PACER_ORIGIN_MS.load(Ordering::Acquire);
    if origin_ms == 0 {
        return;
    }

    let sim_delta_ticks = tick.saturating_sub(origin_tick);
    let target_elapsed_ms = sim_delta_ticks.saturating_mul(1_000) / 60;
    let callback_wait_start_ms = now_ms;

    loop {
        if manual_control_released() {
            return;
        }

        if PRESENTATION_PHASE.load(Ordering::Acquire) == PHASE_PAUSED {
            if !wait_while_paused() {
                return;
            }
            reanchor_pacer();
            return pace_candidate_a(tick);
        }

        if render_heartbeat_stale() {
            if !wait_while_render_stalled() {
                return;
            }
            reanchor_pacer();
            return pace_candidate_a(tick);
        }

        let wall_now_ms = unsafe { GetTickCount64() };
        let wall_elapsed_ms = wall_now_ms.saturating_sub(origin_ms);

        if target_elapsed_ms <= wall_elapsed_ms.saturating_add(ALLOWED_LEAD_MS) {
            return;
        }

        if wall_now_ms.saturating_sub(callback_wait_start_ms) >= MAX_SINGLE_CALLBACK_WAIT_MS {
            // Development builds used to treat this as a permanent fail-open, which silently
            // relinquished manual authority. Recover in place instead: record that the guard
            // fired, re-anchor on the next callback, and keep Direct Control ownership intact.
            SAFETY_FAIL_OPEN.store(true, Ordering::Release);
            reanchor_pacer();
            return;
        }

        let remaining_ms = target_elapsed_ms
            .saturating_sub(wall_elapsed_ms.saturating_add(ALLOWED_LEAD_MS));
        let sleep_ms = remaining_ms.clamp(1, MAX_SLEEP_SLICE_MS);
        PACER_WAIT_COUNT.fetch_add(1, Ordering::Relaxed);
        PACER_TOTAL_WAIT_MS.fetch_add(sleep_ms, Ordering::Relaxed);
        thread::sleep(Duration::from_millis(sleep_ms));
    }
}

impl StablePlayerAi for CandidateAObserverAi {
    fn clone_box(&self) -> Box<dyn StablePlayerAi> {
        Box::new(self.clone())
    }

    fn id(&self) -> String {
        "tfm2_direct_control.candidate_a_observer".to_owned()
    }

    fn priority(&self) -> i32 {
        10_000
    }

    fn matches(&self, _init: &StableAiInit) -> bool {
        true
    }

    fn think(
        &mut self,
        ctx: &mut StableAiContext<'_>,
        base_input: Option<InputV1>,
    ) -> Option<InputV1> {
        TOTAL_THINK_CALLS.fetch_add(1, Ordering::Relaxed);

        let thread_id = unsafe { GetCurrentThreadId() };
        let Some(candidate_a) = candidate_a_probe_for_thread(thread_id) else {
            return base_input;
        };

        observe_candidate_job(candidate_a);

        let tick = ctx.tick() as u64;
        let player_id = ctx.player_id();
        let athlete_id = ctx.athlete_id();

        CANDIDATE_A_THINK_CALLS.fetch_add(1, Ordering::Relaxed);
        let _ = FIRST_CANDIDATE_A_TICK.compare_exchange(
            NO_TICK,
            tick,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        LAST_CANDIDATE_A_TICK.store(tick, Ordering::Relaxed);
        LAST_CANDIDATE_A_PLAYER.store(player_id, Ordering::Relaxed);
        LAST_CANDIDATE_A_ATHLETE.store(athlete_id, Ordering::Relaxed);
        LAST_CANDIDATE_A_THREAD.store(thread_id as u64, Ordering::Relaxed);
        if player_id < u64::BITS as usize {
            SEEN_PLAYER_MASK.fetch_or(1u64 << player_id, Ordering::Relaxed);
        }

        pace_candidate_a(tick);

        if manual_input_enabled() && control::selected_athlete() == Some(athlete_id) {
            // Selected means manual authority. `None` from the control layer means "no manual
            // action this tick", not "let vanilla AI decide instead".
            return control::manual_input_for(ctx, tick);
        }

        base_input
    }
}
