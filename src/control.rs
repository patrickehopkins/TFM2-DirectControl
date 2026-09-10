use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering},
    Mutex,
};

use mod_api_stable::{
    InputV1, SimOriginKindV1, StableAiContext, StableAiInit, StablePlayerAi,
};

const NO_PLAYER: usize = usize::MAX;
const NO_CLOCK: u64 = u64::MAX;

static SELECTED_PLAYER: AtomicUsize = AtomicUsize::new(NO_PLAYER);
static MOVE_ACTIVE: AtomicBool = AtomicBool::new(false);
static MOVE_X: AtomicU64 = AtomicU64::new(0);
static MOVE_Y: AtomicU64 = AtomicU64::new(0);
static MOVE_VERSION: AtomicU64 = AtomicU64::new(0);

// The client overlay publishes the visible match clock once per render frame. On TFM2 0.5.8,
// StablePlayerAi receives an enormous number of callbacks from simulations whose origin is
// reported as Unknown. Matching ctx.tick()/60 to the visible UI second is a narrow bridge to
// the on-screen simulation without feeding client input into every Unknown callback.
static LIVE_CLOCK_SECONDS: AtomicU64 = AtomicU64::new(NO_CLOCK);
static UNKNOWN_CLOCK_MATCHES: AtomicU64 = AtomicU64::new(0);
static LAST_CLOCK_MATCH_TICK: AtomicU64 = AtomicU64::new(NO_CLOCK);

// Runtime diagnostics for the client -> StablePlayerAi boundary.
static THINK_CALLS: AtomicU64 = AtomicU64::new(0);
static SELECTED_THINK_CALLS: AtomicU64 = AtomicU64::new(0);
static MANUAL_MOVE_RETURNS: AtomicU64 = AtomicU64::new(0);
static MANUAL_IDLE_RETURNS: AtomicU64 = AtomicU64::new(0);
static LAST_THINK_PLAYER: AtomicUsize = AtomicUsize::new(NO_PLAYER);
static LAST_ORIGIN_CLASS: AtomicU8 = AtomicU8::new(0);
static SEEN_PLAYER_IDS: Mutex<Vec<usize>> = Mutex::new(Vec::new());

#[derive(Debug, Clone)]
pub struct ControlDiagnostics {
    pub think_calls: u64,
    pub selected_think_calls: u64,
    pub manual_move_returns: u64,
    pub manual_idle_returns: u64,
    pub unknown_clock_matches: u64,
    pub live_clock_seconds: Option<u64>,
    pub last_clock_match_tick: Option<u64>,
    pub last_think_player: Option<usize>,
    pub origin_label: &'static str,
    pub seen_player_ids: Vec<usize>,
}

#[derive(Debug, Clone, Default)]
pub struct DirectControlAi;

fn reset_diagnostics() {
    THINK_CALLS.store(0, Ordering::Release);
    SELECTED_THINK_CALLS.store(0, Ordering::Release);
    MANUAL_MOVE_RETURNS.store(0, Ordering::Release);
    MANUAL_IDLE_RETURNS.store(0, Ordering::Release);
    UNKNOWN_CLOCK_MATCHES.store(0, Ordering::Release);
    LAST_CLOCK_MATCH_TICK.store(NO_CLOCK, Ordering::Release);
    LAST_THINK_PLAYER.store(NO_PLAYER, Ordering::Release);
    LAST_ORIGIN_CLASS.store(0, Ordering::Release);
    if let Ok(mut ids) = SEEN_PLAYER_IDS.lock() {
        ids.clear();
    }
}

pub fn reset() {
    SELECTED_PLAYER.store(NO_PLAYER, Ordering::Release);
    LIVE_CLOCK_SECONDS.store(NO_CLOCK, Ordering::Release);
    clear_move_target();
    reset_diagnostics();
}

pub fn publish_live_clock_seconds(seconds: u64) {
    LIVE_CLOCK_SECONDS.store(seconds, Ordering::Release);
}

pub fn selected_player() -> Option<usize> {
    match SELECTED_PLAYER.load(Ordering::Acquire) {
        NO_PLAYER => None,
        player_id => Some(player_id),
    }
}

pub fn select_player(player_id: usize) {
    // Publish a brief unselected state so a newly selected player can never inherit the
    // previous player's last destination during a cross-thread tick boundary.
    SELECTED_PLAYER.store(NO_PLAYER, Ordering::Release);
    clear_move_target();
    SELECTED_PLAYER.store(player_id, Ordering::Release);
}

pub fn publish_move_target(x: u64, y: u64) {
    // Tiny seqlock: client/render code writes the pair, AI callbacks read it. Direct control
    // intentionally crosses the stable API's deterministic-simulation boundary, so this is
    // single-player only; the seqlock is for thread coherence, not replay determinism.
    MOVE_VERSION.fetch_add(1, Ordering::AcqRel); // odd = write in progress
    MOVE_X.store(x, Ordering::Relaxed);
    MOVE_Y.store(y, Ordering::Relaxed);
    MOVE_ACTIVE.store(true, Ordering::Relaxed);
    MOVE_VERSION.fetch_add(1, Ordering::Release); // even = stable snapshot
}

pub fn clear_move_target() {
    MOVE_VERSION.fetch_add(1, Ordering::AcqRel);
    MOVE_ACTIVE.store(false, Ordering::Relaxed);
    MOVE_X.store(0, Ordering::Relaxed);
    MOVE_Y.store(0, Ordering::Relaxed);
    MOVE_VERSION.fetch_add(1, Ordering::Release);
}

pub fn move_target() -> Option<(u64, u64)> {
    for _ in 0..4 {
        let before = MOVE_VERSION.load(Ordering::Acquire);
        if before & 1 != 0 {
            std::hint::spin_loop();
            continue;
        }

        let active = MOVE_ACTIVE.load(Ordering::Relaxed);
        let x = MOVE_X.load(Ordering::Relaxed);
        let y = MOVE_Y.load(Ordering::Relaxed);
        let after = MOVE_VERSION.load(Ordering::Acquire);

        if before == after && after & 1 == 0 {
            return active.then_some((x, y));
        }
    }

    None
}

pub fn diagnostics() -> ControlDiagnostics {
    let last_think_player = match LAST_THINK_PLAYER.load(Ordering::Acquire) {
        NO_PLAYER => None,
        player_id => Some(player_id),
    };
    let origin_label = match LAST_ORIGIN_CLASS.load(Ordering::Acquire) {
        1 => "ClientMatchView",
        2 => "ClientSpectate",
        3 => "ServerPresim/rejected",
        4 => "ClientReplay/rejected",
        5 => "Tool/rejected",
        6 => "Unknown/rejected",
        7 => "Unknown@clock/accepted",
        8 => "sim unavailable",
        9 => "origin unavailable",
        10 => "unrecognized/rejected",
        _ => "not observed",
    };
    let seen_player_ids = SEEN_PLAYER_IDS
        .lock()
        .map(|ids| ids.clone())
        .unwrap_or_default();
    let live_clock_seconds = match LIVE_CLOCK_SECONDS.load(Ordering::Acquire) {
        NO_CLOCK => None,
        seconds => Some(seconds),
    };
    let last_clock_match_tick = match LAST_CLOCK_MATCH_TICK.load(Ordering::Acquire) {
        NO_CLOCK => None,
        tick => Some(tick),
    };

    ControlDiagnostics {
        think_calls: THINK_CALLS.load(Ordering::Acquire),
        selected_think_calls: SELECTED_THINK_CALLS.load(Ordering::Acquire),
        manual_move_returns: MANUAL_MOVE_RETURNS.load(Ordering::Acquire),
        manual_idle_returns: MANUAL_IDLE_RETURNS.load(Ordering::Acquire),
        unknown_clock_matches: UNKNOWN_CLOCK_MATCHES.load(Ordering::Acquire),
        live_clock_seconds,
        last_clock_match_tick,
        last_think_player,
        origin_label,
        seen_player_ids,
    }
}

fn note_player_id(player_id: usize) {
    if let Ok(mut ids) = SEEN_PLAYER_IDS.lock() {
        if !ids.contains(&player_id) {
            ids.push(player_id);
            ids.sort_unstable();
        }
    }
}

impl StablePlayerAi for DirectControlAi {
    fn clone_box(&self) -> Box<dyn StablePlayerAi> {
        Box::new(self.clone())
    }

    fn id(&self) -> String {
        "tfm2_direct_control.manual_input".to_owned()
    }

    fn priority(&self) -> i32 {
        10_000
    }

    fn matches(&self, _init: &StableAiInit) -> bool {
        // Attach to every player so F1-F10 can select any of the ten visible match slots.
        // When a player is not selected, think() returns the engine's base input unchanged.
        true
    }

    fn think(
        &mut self,
        ctx: &mut StableAiContext<'_>,
        base_input: Option<InputV1>,
    ) -> Option<InputV1> {
        // Capture immutable context values before borrowing ctx mutably through sim().
        let player_id = ctx.player_id();
        let tick = ctx.tick() as u64;
        THINK_CALLS.fetch_add(1, Ordering::Relaxed);
        LAST_THINK_PLAYER.store(player_id, Ordering::Relaxed);
        note_player_id(player_id);

        if selected_player() != Some(player_id) {
            return base_input;
        }
        SELECTED_THINK_CALLS.fetch_add(1, Ordering::Relaxed);

        let Some(sim) = ctx.sim() else {
            LAST_ORIGIN_CLASS.store(8, Ordering::Relaxed);
            return base_input;
        };
        let Some(origin) = sim.sim_origin() else {
            LAST_ORIGIN_CLASS.store(9, Ordering::Relaxed);
            return base_input;
        };

        let is_match_view = origin.kind == SimOriginKindV1::ClientMatchView.code();
        let is_spectate = origin.kind == SimOriginKindV1::ClientSpectate.code();
        let is_server_presim = origin.kind == SimOriginKindV1::ServerPresim.code();
        let is_replay = origin.kind == SimOriginKindV1::ClientReplay.code();
        let is_tool = origin.kind == SimOriginKindV1::Tool.code();
        let is_unknown = origin.kind == SimOriginKindV1::Unknown.code();

        let live_clock = LIVE_CLOCK_SECONDS.load(Ordering::Acquire);
        let unknown_matches_clock = is_unknown && live_clock != NO_CLOCK && tick / 60 == live_clock;

        LAST_ORIGIN_CLASS.store(
            if is_match_view {
                1
            } else if is_spectate {
                2
            } else if is_server_presim {
                3
            } else if is_replay {
                4
            } else if is_tool {
                5
            } else if unknown_matches_clock {
                7
            } else if is_unknown {
                6
            } else {
                10
            },
            Ordering::Relaxed,
        );

        // Named presentation-side client origins are accepted directly. TFM2 0.5.8 reports
        // the dominant live-match StablePlayerAi traffic as Unknown, so Unknown is accepted
        // only when its simulation tick falls in the exact second currently displayed by the
        // live UI clock. Server presims, replays, tools, other Unknown ticks, and unrecognized
        // origins remain untouched.
        if !is_match_view && !is_spectate && !unknown_matches_clock {
            return base_input;
        }

        if unknown_matches_clock {
            UNKNOWN_CLOCK_MATCHES.fetch_add(1, Ordering::Relaxed);
            LAST_CLOCK_MATCH_TICK.store(tick, Ordering::Relaxed);
        }

        if let Some((x, y)) = move_target() {
            MANUAL_MOVE_RETURNS.fetch_add(1, Ordering::Relaxed);
            return Some(InputV1::move_to(x, y));
        }

        // Selection itself should hand control to the user immediately, not let the selected
        // champion keep following vanilla AI until the first click. A move-to-self input is
        // the least invasive idle/stop command available through the stable API.
        let Some(player) = sim.get_player(player_id) else {
            return base_input;
        };
        let Some(champion) = player.champion() else {
            return base_input;
        };
        let (x, y) = champion.pos();

        MANUAL_IDLE_RETURNS.fetch_add(1, Ordering::Relaxed);
        Some(InputV1::move_to(x, y))
    }
}
