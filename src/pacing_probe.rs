//! Continuous wall-clock pacing proof for the confirmed Candidate A watched-match simulation.
//!
//! Stage 3A physically proved that StablePlayerAi can cooperatively hold Candidate A at ~60
//! simulation ticks per wall-clock second while visible playback continues normally. The earlier
//! build intentionally released after 30 seconds as a safety boundary; this follow-up removes only
//! that artificial timeout so we can verify that Candidate A stays live for the full match.
//!
//! Safety boundaries remain:
//! - only callbacks running on the confirmed Candidate A worker thread are delayed;
//! - `base_input` is returned unchanged, so player behavior remains AI-controlled;
//! - a ~35 ms lead is allowed to avoid excessive one-millisecond sleep jitter;
//! - if any single callback would have to wait 250 ms, the pacer permanently fails open rather
//!   than risking a stuck worker;
//! - no replay/presentation object is read or written here.
//!
//! This is deliberately a fixed 1x / 60 Hz pacing test. Playback-speed and pause synchronization
//! are separate concerns to solve after full-match pacing is physically validated.

use std::{
    sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    thread,
    time::Duration,
};

use mod_api_stable::{InputV1, StableAiContext, StableAiInit, StablePlayerAi};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;

use crate::simulation_probe;

const NO_TICK: u64 = u64::MAX;
const NO_PLAYER: usize = usize::MAX;
const ALLOWED_LEAD_MS: u64 = 35;
const MAX_SLEEP_SLICE_MS: u64 = 2;
const MAX_SINGLE_CALLBACK_WAIT_MS: u64 = 250;

#[link(name = "kernel32")]
extern "system" {
    fn GetTickCount64() -> u64;
}

static TOTAL_THINK_CALLS: AtomicU64 = AtomicU64::new(0);
static CANDIDATE_A_THINK_CALLS: AtomicU64 = AtomicU64::new(0);
static FIRST_CANDIDATE_A_TICK: AtomicU64 = AtomicU64::new(NO_TICK);
static LAST_CANDIDATE_A_TICK: AtomicU64 = AtomicU64::new(NO_TICK);
static LAST_CANDIDATE_A_PLAYER: AtomicUsize = AtomicUsize::new(NO_PLAYER);
static LAST_CANDIDATE_A_THREAD: AtomicU64 = AtomicU64::new(0);
static SEEN_PLAYER_MASK: AtomicU64 = AtomicU64::new(0);

static PACER_ORIGIN_TICK: AtomicU64 = AtomicU64::new(NO_TICK);
static PACER_ORIGIN_MS: AtomicU64 = AtomicU64::new(0);
static PACER_RELEASED: AtomicBool = AtomicBool::new(false);
static PACER_WAIT_COUNT: AtomicU64 = AtomicU64::new(0);
static PACER_TOTAL_WAIT_MS: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy)]
pub struct PacingProbeSnapshot {
    pub total_think_calls: u64,
    pub candidate_a_think_calls: u64,
    pub first_candidate_a_tick: Option<u64>,
    pub last_candidate_a_tick: Option<u64>,
    pub last_candidate_a_player: Option<usize>,
    pub last_candidate_a_thread: u32,
    pub seen_player_mask: u64,
    pub pacer_origin_tick: Option<u64>,
    pub pacer_elapsed_ms: u64,
    pub pacer_released: bool,
    pub pacer_wait_count: u64,
    pub pacer_total_wait_ms: u64,
}

#[derive(Debug, Clone, Default)]
pub struct CandidateAObserverAi;

pub fn reset() {
    TOTAL_THINK_CALLS.store(0, Ordering::Release);
    CANDIDATE_A_THINK_CALLS.store(0, Ordering::Release);
    FIRST_CANDIDATE_A_TICK.store(NO_TICK, Ordering::Release);
    LAST_CANDIDATE_A_TICK.store(NO_TICK, Ordering::Release);
    LAST_CANDIDATE_A_PLAYER.store(NO_PLAYER, Ordering::Release);
    LAST_CANDIDATE_A_THREAD.store(0, Ordering::Release);
    SEEN_PLAYER_MASK.store(0, Ordering::Release);

    PACER_ORIGIN_TICK.store(NO_TICK, Ordering::Release);
    PACER_ORIGIN_MS.store(0, Ordering::Release);
    PACER_RELEASED.store(false, Ordering::Release);
    PACER_WAIT_COUNT.store(0, Ordering::Release);
    PACER_TOTAL_WAIT_MS.store(0, Ordering::Release);
}

pub fn snapshot() -> PacingProbeSnapshot {
    let first_tick = FIRST_CANDIDATE_A_TICK.load(Ordering::Acquire);
    let last_tick = LAST_CANDIDATE_A_TICK.load(Ordering::Acquire);
    let last_player = LAST_CANDIDATE_A_PLAYER.load(Ordering::Acquire);
    let pacer_origin_tick = PACER_ORIGIN_TICK.load(Ordering::Acquire);
    let pacer_origin_ms = PACER_ORIGIN_MS.load(Ordering::Acquire);
    let now_ms = unsafe { GetTickCount64() };

    PacingProbeSnapshot {
        total_think_calls: TOTAL_THINK_CALLS.load(Ordering::Acquire),
        candidate_a_think_calls: CANDIDATE_A_THINK_CALLS.load(Ordering::Acquire),
        first_candidate_a_tick: (first_tick != NO_TICK).then_some(first_tick),
        last_candidate_a_tick: (last_tick != NO_TICK).then_some(last_tick),
        last_candidate_a_player: (last_player != NO_PLAYER).then_some(last_player),
        last_candidate_a_thread: LAST_CANDIDATE_A_THREAD.load(Ordering::Acquire) as u32,
        seen_player_mask: SEEN_PLAYER_MASK.load(Ordering::Acquire),
        pacer_origin_tick: (pacer_origin_tick != NO_TICK).then_some(pacer_origin_tick),
        pacer_elapsed_ms: if pacer_origin_ms == 0 {
            0
        } else {
            now_ms.saturating_sub(pacer_origin_ms)
        },
        pacer_released: PACER_RELEASED.load(Ordering::Acquire),
        pacer_wait_count: PACER_WAIT_COUNT.load(Ordering::Acquire),
        pacer_total_wait_ms: PACER_TOTAL_WAIT_MS.load(Ordering::Acquire),
    }
}

fn running_on_candidate_a_thread(thread_id: u32) -> bool {
    let candidate_a = simulation_probe::snapshots()[0];
    candidate_a.active != 0
        && candidate_a.last_thread_id != 0
        && candidate_a.last_thread_id == thread_id
}

fn pace_candidate_a(tick: u64) {
    if PACER_RELEASED.load(Ordering::Acquire) {
        return;
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
        let wall_now_ms = unsafe { GetTickCount64() };
        let wall_elapsed_ms = wall_now_ms.saturating_sub(origin_ms);

        if target_elapsed_ms <= wall_elapsed_ms.saturating_add(ALLOWED_LEAD_MS) {
            return;
        }

        if wall_now_ms.saturating_sub(callback_wait_start_ms) >= MAX_SINGLE_CALLBACK_WAIT_MS {
            // A normal 60 Hz tick should need only a few milliseconds of waiting. If one callback
            // ever wants hundreds of milliseconds, fail open rather than risking a stuck worker.
            PACER_RELEASED.store(true, Ordering::Release);
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
        if running_on_candidate_a_thread(thread_id) {
            let tick = ctx.tick() as u64;
            let player_id = ctx.player_id();

            CANDIDATE_A_THINK_CALLS.fetch_add(1, Ordering::Relaxed);
            let _ = FIRST_CANDIDATE_A_TICK.compare_exchange(
                NO_TICK,
                tick,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            LAST_CANDIDATE_A_TICK.store(tick, Ordering::Relaxed);
            LAST_CANDIDATE_A_PLAYER.store(player_id, Ordering::Relaxed);
            LAST_CANDIDATE_A_THREAD.store(thread_id as u64, Ordering::Relaxed);
            if player_id < u64::BITS as usize {
                SEEN_PLAYER_MASK.fetch_or(1u64 << player_id, Ordering::Relaxed);
            }

            pace_candidate_a(tick);
        }

        // Pacing proof only: preserve exactly the AI input passed to this hook.
        base_input
    }
}
