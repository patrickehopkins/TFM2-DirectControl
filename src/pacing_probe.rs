//! Live-control pacing for the confirmed Candidate A watched-match simulation.
//!
//! Stage 3 proved Candidate A can be paced at ~60 ticks/s and irreversibly released with Ctrl+End.
//! Stage 4 proved manual `InputV1::move_to` commands reach that live simulation.
//!
//! Startup probing on v0.6.1 proved Candidate A is already the watched ClientMatchView simulation
//! at tick 1. It also proved the loader behaves materially better with the existing bounded startup
//! hold: tick 1 completes, the next tick waits up to two seconds, then Candidate A resumes at 60 Hz.
//!
//! The loader auto-release is now distinct from the user's Ctrl+Home start. It opens only enough
//! runway for the client to reach its first InGame frame. That frame latches the current simulation
//! tick; the tick is allowed to finish, then Candidate A blocks again until the user explicitly
//! starts Direct Control. This preserves the loader's physically validated path while preventing
//! further watched-match progress once the battlefield is actually available.
//! The worker-local Ctrl+Home escape remains available while a Candidate-A callback is held.
//!
//! Pause uses a separate presentation gate. Ctrl+End permanently releases pacing and manual input
//! for the current match.

use std::{
    sync::atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering},
    thread,
    time::Duration,
};

use mod_api_stable::{
    InputV1, SimOriginKindV1, StableAiContext, StableAiInit, StablePlayerAi,
};
use windows_sys::Win32::{
    System::Threading::GetCurrentThreadId,
    UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_HOME},
};

use crate::{control, input_focus, simulation_probe};

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

// Read-only pregame readiness probe. None of these fields affect pacing or control.
static STARTUP_JOB_START_MS: AtomicU64 = AtomicU64::new(0);
static LAST_ORIGIN_PROBE_TICK: AtomicU64 = AtomicU64::new(NO_TICK);
static FIRST_ORIGIN_KIND: AtomicU64 = AtomicU64::new(u64::MAX);
static FIRST_ORIGIN_TICK: AtomicU64 = AtomicU64::new(NO_TICK);
static LAST_ORIGIN_KIND: AtomicU64 = AtomicU64::new(u64::MAX);
static FIRST_CLIENT_MATCH_VIEW_TICK: AtomicU64 = AtomicU64::new(NO_TICK);
static FIRST_CLIENT_MATCH_VIEW_MS: AtomicU64 = AtomicU64::new(NO_TICK);
static CLIENT_MATCH_VIEW_MATCH_ID: AtomicU64 = AtomicU64::new(u64::MAX);
static CLIENT_MATCH_VIEW_REPLAY_ID: AtomicU64 = AtomicU64::new(u64::MAX);
static CLIENT_MATCH_VIEW_SET_INDEX: AtomicU64 = AtomicU64::new(u64::MAX);
static FIRST_MATCH_RENDER_TICK: AtomicU64 = AtomicU64::new(NO_TICK);
static FIRST_MATCH_RENDER_MS: AtomicU64 = AtomicU64::new(NO_TICK);
static FIRST_GAME_MAP_TICK: AtomicU64 = AtomicU64::new(NO_TICK);
static FIRST_GAME_MAP_MS: AtomicU64 = AtomicU64::new(NO_TICK);
static FIRST_CENTER_LOG_TICK: AtomicU64 = AtomicU64::new(NO_TICK);
static FIRST_CENTER_LOG_MS: AtomicU64 = AtomicU64::new(NO_TICK);
static FIRST_INGAME_RENDER_TICK: AtomicU64 = AtomicU64::new(NO_TICK);
static FIRST_INGAME_RENDER_MS: AtomicU64 = AtomicU64::new(NO_TICK);

static START_REQUESTED: AtomicBool = AtomicBool::new(false);
static START_AUTO_RELEASED: AtomicBool = AtomicBool::new(false);
static INTERACTIVE_MATCH: AtomicBool = AtomicBool::new(false);
static LAST_RENDER_HEARTBEAT_MS: AtomicU64 = AtomicU64::new(0);
static PRESENTATION_PHASE: AtomicU8 = AtomicU8::new(PHASE_WAITING_START);
// When the client first reaches InGame, finish this currently observed simulation tick before
// blocking. That avoids freezing halfway through a ten-player tick while still preventing any
// further watched-match progress before the user's explicit Ctrl+Home.
static READY_GATE_TICK: AtomicU64 = AtomicU64::new(NO_TICK);
static STARTUP_PRESENTATION_SYNCED: AtomicBool = AtomicBool::new(false);
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
static WORKER_START_CHORD_WAS_DOWN: AtomicBool = AtomicBool::new(false);

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
    pub first_origin_kind: Option<u64>,
    pub first_origin_tick: Option<u64>,
    pub startup_origin_kind: Option<u64>,
    pub client_match_view_tick: Option<u64>,
    pub client_match_view_ms: Option<u64>,
    pub client_match_view_match_id: Option<u64>,
    pub client_match_view_replay_id: Option<u64>,
    pub client_match_view_set_index: Option<u64>,
    pub first_match_render_tick: Option<u64>,
    pub first_match_render_ms: Option<u64>,
    pub first_game_map_tick: Option<u64>,
    pub first_game_map_ms: Option<u64>,
    pub first_center_log_tick: Option<u64>,
    pub first_center_log_ms: Option<u64>,
    pub first_ingame_render_tick: Option<u64>,
    pub first_ingame_render_ms: Option<u64>,
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
    READY_GATE_TICK.store(NO_TICK, Ordering::Release);
    STARTUP_PRESENTATION_SYNCED.store(false, Ordering::Release);
    reanchor_pacer();
    MANUAL_FINISH_REQUESTED.store(false, Ordering::Release);
    SAFETY_FAIL_OPEN.store(false, Ordering::Release);
    PACER_WAIT_COUNT.store(0, Ordering::Release);
    PACER_TOTAL_WAIT_MS.store(0, Ordering::Release);
    START_WAIT_COUNT.store(0, Ordering::Release);
    START_TOTAL_WAIT_MS.store(0, Ordering::Release);
    PAUSE_WAIT_COUNT.store(0, Ordering::Release);
    PAUSE_TOTAL_WAIT_MS.store(0, Ordering::Release);
    WORKER_START_CHORD_WAS_DOWN.store(false, Ordering::Release);

    STARTUP_JOB_START_MS.store(0, Ordering::Release);
    LAST_ORIGIN_PROBE_TICK.store(NO_TICK, Ordering::Release);
    FIRST_ORIGIN_KIND.store(u64::MAX, Ordering::Release);
    FIRST_ORIGIN_TICK.store(NO_TICK, Ordering::Release);
    LAST_ORIGIN_KIND.store(u64::MAX, Ordering::Release);
    FIRST_CLIENT_MATCH_VIEW_TICK.store(NO_TICK, Ordering::Release);
    FIRST_CLIENT_MATCH_VIEW_MS.store(NO_TICK, Ordering::Release);
    CLIENT_MATCH_VIEW_MATCH_ID.store(u64::MAX, Ordering::Release);
    CLIENT_MATCH_VIEW_REPLAY_ID.store(u64::MAX, Ordering::Release);
    CLIENT_MATCH_VIEW_SET_INDEX.store(u64::MAX, Ordering::Release);
    FIRST_MATCH_RENDER_TICK.store(NO_TICK, Ordering::Release);
    FIRST_MATCH_RENDER_MS.store(NO_TICK, Ordering::Release);
    FIRST_GAME_MAP_TICK.store(NO_TICK, Ordering::Release);
    FIRST_GAME_MAP_MS.store(NO_TICK, Ordering::Release);
    FIRST_CENTER_LOG_TICK.store(NO_TICK, Ordering::Release);
    FIRST_CENTER_LOG_MS.store(NO_TICK, Ordering::Release);
    FIRST_INGAME_RENDER_TICK.store(NO_TICK, Ordering::Release);
    FIRST_INGAME_RENDER_MS.store(NO_TICK, Ordering::Release);
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
    STARTUP_JOB_START_MS.store(unsafe { GetTickCount64() }, Ordering::Release);
}

fn startup_elapsed_ms() -> Option<u64> {
    let start = STARTUP_JOB_START_MS.load(Ordering::Acquire);
    (start != 0).then(|| unsafe { GetTickCount64() }.saturating_sub(start))
}

fn record_client_milestone(tick_slot: &AtomicU64, ms_slot: &AtomicU64) {
    if tick_slot.load(Ordering::Acquire) != NO_TICK {
        return;
    }
    let Some(elapsed_ms) = startup_elapsed_ms() else {
        return;
    };
    let tick = LAST_CANDIDATE_A_TICK.load(Ordering::Acquire);
    ms_slot.store(elapsed_ms, Ordering::Relaxed);
    let _ = tick_slot.compare_exchange(NO_TICK, tick, Ordering::AcqRel, Ordering::Acquire);
}

pub fn note_match_render() {
    record_client_milestone(&FIRST_MATCH_RENDER_TICK, &FIRST_MATCH_RENDER_MS);
}

pub fn note_game_map_ready() {
    record_client_milestone(&FIRST_GAME_MAP_TICK, &FIRST_GAME_MAP_MS);
}

pub fn note_center_log_ready() {
    record_client_milestone(&FIRST_CENTER_LOG_TICK, &FIRST_CENTER_LOG_MS);
}

pub fn note_ingame_render() {
    record_client_milestone(&FIRST_INGAME_RENDER_TICK, &FIRST_INGAME_RENDER_MS);
}

fn observe_sim_origin(ctx: &mut StableAiContext<'_>, tick: u64) {
    if LAST_ORIGIN_PROBE_TICK.swap(tick, Ordering::AcqRel) == tick {
        return;
    }

    let Some(sim) = ctx.sim() else {
        return;
    };
    let Some(origin) = sim.sim_origin() else {
        return;
    };

    let origin_kind = origin.kind as u64;
    LAST_ORIGIN_KIND.store(origin_kind, Ordering::Release);
    if FIRST_ORIGIN_TICK.load(Ordering::Acquire) == NO_TICK {
        FIRST_ORIGIN_KIND.store(origin_kind, Ordering::Relaxed);
        let _ = FIRST_ORIGIN_TICK.compare_exchange(
            NO_TICK,
            tick,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    if origin.kind != SimOriginKindV1::ClientMatchView.code()
        || FIRST_CLIENT_MATCH_VIEW_TICK.load(Ordering::Acquire) != NO_TICK
    {
        return;
    }

    CLIENT_MATCH_VIEW_MATCH_ID.store(origin.match_id, Ordering::Relaxed);
    CLIENT_MATCH_VIEW_REPLAY_ID.store(origin.replay_id, Ordering::Relaxed);
    CLIENT_MATCH_VIEW_SET_INDEX.store(origin.set_index, Ordering::Relaxed);
    FIRST_CLIENT_MATCH_VIEW_MS.store(startup_elapsed_ms().unwrap_or(NO_TICK), Ordering::Relaxed);
    let _ = FIRST_CLIENT_MATCH_VIEW_TICK.compare_exchange(
        NO_TICK,
        tick,
        Ordering::AcqRel,
        Ordering::Acquire,
    );
}

pub fn ready_gate_tick() -> Option<u64> {
    let tick = READY_GATE_TICK.load(Ordering::Acquire);
    (tick != NO_TICK).then_some(tick)
}

pub fn set_startup_presentation_synced(synced: bool) {
    STARTUP_PRESENTATION_SYNCED.store(synced, Ordering::Release);
}

pub fn startup_presentation_synced() -> bool {
    STARTUP_PRESENTATION_SYNCED.load(Ordering::Acquire)
}

/// Starts the held Candidate-A simulation and re-anchors the 60 Hz wall-clock pacer.
pub fn request_start_simulation() {
    if manual_control_released() {
        return;
    }

    if INTERACTIVE_MATCH.load(Ordering::Acquire)
        && !STARTUP_PRESENTATION_SYNCED.load(Ordering::Acquire)
    {
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
    let was_interactive = INTERACTIVE_MATCH.swap(interactive_match, Ordering::AcqRel);

    if interactive_match
        && !was_interactive
        && !START_REQUESTED.load(Ordering::Acquire)
        && READY_GATE_TICK.load(Ordering::Acquire) == NO_TICK
    {
        READY_GATE_TICK.store(LAST_CANDIDATE_A_TICK.load(Ordering::Acquire), Ordering::Release);
    }

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
        first_origin_kind: {
            let value = FIRST_ORIGIN_KIND.load(Ordering::Acquire);
            (value != u64::MAX).then_some(value)
        },
        first_origin_tick: {
            let value = FIRST_ORIGIN_TICK.load(Ordering::Acquire);
            (value != NO_TICK).then_some(value)
        },
        startup_origin_kind: {
            let value = LAST_ORIGIN_KIND.load(Ordering::Acquire);
            (value != u64::MAX).then_some(value)
        },
        client_match_view_tick: {
            let value = FIRST_CLIENT_MATCH_VIEW_TICK.load(Ordering::Acquire);
            (value != NO_TICK).then_some(value)
        },
        client_match_view_ms: {
            let value = FIRST_CLIENT_MATCH_VIEW_MS.load(Ordering::Acquire);
            (value != NO_TICK).then_some(value)
        },
        client_match_view_match_id: {
            let value = CLIENT_MATCH_VIEW_MATCH_ID.load(Ordering::Acquire);
            (value != u64::MAX).then_some(value)
        },
        client_match_view_replay_id: {
            let value = CLIENT_MATCH_VIEW_REPLAY_ID.load(Ordering::Acquire);
            (value != u64::MAX).then_some(value)
        },
        client_match_view_set_index: {
            let value = CLIENT_MATCH_VIEW_SET_INDEX.load(Ordering::Acquire);
            (value != u64::MAX).then_some(value)
        },
        first_match_render_tick: {
            let value = FIRST_MATCH_RENDER_TICK.load(Ordering::Acquire);
            (value != NO_TICK).then_some(value)
        },
        first_match_render_ms: {
            let value = FIRST_MATCH_RENDER_MS.load(Ordering::Acquire);
            (value != NO_TICK).then_some(value)
        },
        first_game_map_tick: {
            let value = FIRST_GAME_MAP_TICK.load(Ordering::Acquire);
            (value != NO_TICK).then_some(value)
        },
        first_game_map_ms: {
            let value = FIRST_GAME_MAP_MS.load(Ordering::Acquire);
            (value != NO_TICK).then_some(value)
        },
        first_center_log_tick: {
            let value = FIRST_CENTER_LOG_TICK.load(Ordering::Acquire);
            (value != NO_TICK).then_some(value)
        },
        first_center_log_ms: {
            let value = FIRST_CENTER_LOG_MS.load(Ordering::Acquire);
            (value != NO_TICK).then_some(value)
        },
        first_ingame_render_tick: {
            let value = FIRST_INGAME_RENDER_TICK.load(Ordering::Acquire);
            (value != NO_TICK).then_some(value)
        },
        first_ingame_render_ms: {
            let value = FIRST_INGAME_RENDER_MS.load(Ordering::Acquire);
            (value != NO_TICK).then_some(value)
        },
    }
}

fn candidate_a_probe_for_thread(thread_id: u32) -> Option<simulation_probe::SimulationProbeSnapshot> {
    let candidate_a = simulation_probe::snapshots()[0];
    (candidate_a.active != 0
        && candidate_a.last_thread_id != 0
        && candidate_a.last_thread_id == thread_id)
        .then_some(candidate_a)
}

fn ctrl_home_pressed_in_foreground() -> bool {
    if !input_focus::process_owns_foreground_window() {
        // Swallow a Ctrl+Home that was pressed while another application owned focus. The chord
        // must be released and pressed again after TFM2 becomes foreground before it can count.
        WORKER_START_CHORD_WAS_DOWN.store(true, Ordering::Release);
        return false;
    }

    let chord_down = unsafe {
        GetAsyncKeyState(VK_CONTROL as i32) < 0 && GetAsyncKeyState(VK_HOME as i32) < 0
    };
    let was_down = WORKER_START_CHORD_WAS_DOWN.swap(chord_down, Ordering::AcqRel);
    chord_down && !was_down
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
        if ctrl_home_pressed_in_foreground() {
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
            // Loader escape only: allow Candidate A to resume at 60 Hz, but keep manual control
            // unstarted so the first real InGame frame can re-establish a deliberate Ctrl+Home gate.
            START_AUTO_RELEASED.store(true, Ordering::Release);
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
        if INTERACTIVE_MATCH.load(Ordering::Acquire) {
            let ready_tick = READY_GATE_TICK.load(Ordering::Acquire);

            // Finish the simulation tick already in progress when InGame first becomes visible.
            // The next tick blocks for the user's actual Ctrl+Home.
            if ready_tick != NO_TICK && tick <= ready_tick {
                return;
            }

            if !wait_until_started() {
                return;
            }

            reanchor_pacer();
            return pace_candidate_a(tick);
        }

        if !START_AUTO_RELEASED.load(Ordering::Acquire) {
            let first_tick = FIRST_CANDIDATE_A_TICK.load(Ordering::Acquire);

            // Preserve the loader behavior that physically reached InGame around tick 166:
            // complete tick 1, hold at the next tick for the bounded 2-second runway gate, then
            // resume Candidate A at 60 Hz without treating that recovery as the user's start.
            if first_tick == NO_TICK || tick <= first_tick {
                return;
            }

            if !wait_until_started() {
                return;
            }

            // Ctrl+Home may have been pressed during the hold. If so, START_REQUESTED is now true
            // and the recursive call enters ordinary live pacing. Otherwise this was only the
            // bounded loader auto-release and the next call falls through to the 60 Hz loader runway.
            reanchor_pacer();
            return pace_candidate_a(tick);
        }

        // Loader runway: paced at the proven 60 Hz until the first InGame frame is observed.
        // START_REQUESTED remains false, so that frame immediately reinstates the user gate.
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

        observe_sim_origin(ctx, tick);
        pace_candidate_a(tick);

        // Debug-only click geometry is intentionally throttled; targeting itself remains full-rate.
        if manual_input_enabled() {
            control::refresh_click_target_overlay(ctx, tick);
        }

        if manual_input_enabled() && control::selected_athlete() == Some(athlete_id) {
            // Selected means manual authority. `None` from the control layer means "no manual
            // action this tick", not "let vanilla AI decide instead".
            return control::manual_input_for(ctx, tick);
        }

        base_input
    }
}
