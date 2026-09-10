//! Read-only StablePlayerAi observer for the confirmed Candidate A watched-match job.
//!
//! This probe deliberately returns `base_input` unchanged. Its only purpose is to prove that
//! StablePlayerAi callbacks running on Candidate A's worker thread expose the authoritative
//! simulation tick we need for later pacing. No sleeping, player selection, or manual input is
//! enabled here.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use mod_api_stable::{InputV1, StableAiContext, StableAiInit, StablePlayerAi};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;

use crate::simulation_probe;

const NO_TICK: u64 = u64::MAX;
const NO_PLAYER: usize = usize::MAX;

static TOTAL_THINK_CALLS: AtomicU64 = AtomicU64::new(0);
static CANDIDATE_A_THINK_CALLS: AtomicU64 = AtomicU64::new(0);
static FIRST_CANDIDATE_A_TICK: AtomicU64 = AtomicU64::new(NO_TICK);
static LAST_CANDIDATE_A_TICK: AtomicU64 = AtomicU64::new(NO_TICK);
static LAST_CANDIDATE_A_PLAYER: AtomicUsize = AtomicUsize::new(NO_PLAYER);
static LAST_CANDIDATE_A_THREAD: AtomicU64 = AtomicU64::new(0);
static SEEN_PLAYER_MASK: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy)]
pub struct PacingProbeSnapshot {
    pub total_think_calls: u64,
    pub candidate_a_think_calls: u64,
    pub first_candidate_a_tick: Option<u64>,
    pub last_candidate_a_tick: Option<u64>,
    pub last_candidate_a_player: Option<usize>,
    pub last_candidate_a_thread: u32,
    pub seen_player_mask: u64,
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
}

pub fn snapshot() -> PacingProbeSnapshot {
    let first_tick = FIRST_CANDIDATE_A_TICK.load(Ordering::Acquire);
    let last_tick = LAST_CANDIDATE_A_TICK.load(Ordering::Acquire);
    let last_player = LAST_CANDIDATE_A_PLAYER.load(Ordering::Acquire);

    PacingProbeSnapshot {
        total_think_calls: TOTAL_THINK_CALLS.load(Ordering::Acquire),
        candidate_a_think_calls: CANDIDATE_A_THINK_CALLS.load(Ordering::Acquire),
        first_candidate_a_tick: (first_tick != NO_TICK).then_some(first_tick),
        last_candidate_a_tick: (last_tick != NO_TICK).then_some(last_tick),
        last_candidate_a_player: (last_player != NO_PLAYER).then_some(last_player),
        last_candidate_a_thread: LAST_CANDIDATE_A_THREAD.load(Ordering::Acquire) as u32,
        seen_player_mask: SEEN_PLAYER_MASK.load(Ordering::Acquire),
    }
}

fn running_on_candidate_a_thread(thread_id: u32) -> bool {
    let candidate_a = simulation_probe::snapshots()[0];
    candidate_a.active != 0
        && candidate_a.last_thread_id != 0
        && candidate_a.last_thread_id == thread_id
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
        // Observe every player for this validation build so we do not assume a particular player-id
        // assignment. The thread filter below isolates callbacks belonging to Candidate A.
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
        }

        // Diagnostic only: preserve exactly the input passed to this hook.
        base_input
    }
}
