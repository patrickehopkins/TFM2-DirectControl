use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering},
    Mutex,
};

use mod_api_stable::{
    InputV1, SimOriginKindV1, StableAiContext, StableAiInit, StablePlayerAi,
};

const NO_PLAYER: usize = usize::MAX;

static SELECTED_PLAYER: AtomicUsize = AtomicUsize::new(NO_PLAYER);
static MOVE_ACTIVE: AtomicBool = AtomicBool::new(false);
static MOVE_X: AtomicU64 = AtomicU64::new(0);
static MOVE_Y: AtomicU64 = AtomicU64::new(0);
static MOVE_VERSION: AtomicU64 = AtomicU64::new(0);

// Runtime diagnostics for the client -> StablePlayerAi boundary. These are intentionally
// small atomics plus a short unique-ID list so the UI can tell us whether the AI callback is
// running for the player IDs we expect and which simulation origin it sees.
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
    LAST_THINK_PLAYER.store(NO_PLAYER, Ordering::Release);
    LAST_ORIGIN_CLASS.store(0, Ordering::Release);
    if let Ok(mut ids) = SEEN_PLAYER_IDS.lock() {
        ids.clear();
    }
}

pub fn reset() {
    SELECTED_PLAYER.store(NO_PLAYER, Ordering::Release);
    clear_move_target();
    reset_diagnostics();
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
        3 => "ServerPresim",
        4 => "ClientReplay/rejected",
        5 => "Tool/rejected",
        6 => "Unknown/rejected",
        7 => "sim unavailable",
        8 => "origin unavailable",
        9 => "unrecognized/rejected",
        _ => "not observed",
    };
    let seen_player_ids = SEEN_PLAYER_IDS
        .lock()
        .map(|ids| ids.clone())
        .unwrap_or_default();

    ControlDiagnostics {
        think_calls: THINK_CALLS.load(Ordering::Acquire),
        selected_think_calls: SELECTED_THINK_CALLS.load(Ordering::Acquire),
        manual_move_returns: MANUAL_MOVE_RETURNS.load(Ordering::Acquire),
        manual_idle_returns: MANUAL_IDLE_RETURNS.load(Ordering::Acquire),
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
        THINK_CALLS.fetch_add(1, Ordering::Relaxed);
        LAST_THINK_PLAYER.store(player_id, Ordering::Relaxed);
        note_player_id(player_id);

        if selected_player() != Some(player_id) {
            return base_input;
        }
        SELECTED_THINK_CALLS.fetch_add(1, Ordering::Relaxed);

        let Some(sim) = ctx.sim() else {
            LAST_ORIGIN_CLASS.store(7, Ordering::Relaxed);
            return base_input;
        };
        let Some(origin) = sim.sim_origin() else {
            LAST_ORIGIN_CLASS.store(8, Ordering::Relaxed);
            return base_input;
        };

        let is_match_view = origin.kind == SimOriginKindV1::ClientMatchView.code();
        let is_spectate = origin.kind == SimOriginKindV1::ClientSpectate.code();
        let is_server_presim = origin.kind == SimOriginKindV1::ServerPresim.code();
        let is_replay = origin.kind == SimOriginKindV1::ClientReplay.code();
        let is_tool = origin.kind == SimOriginKindV1::Tool.code();
        let is_unknown = origin.kind == SimOriginKindV1::Unknown.code();

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
            } else if is_unknown {
                6
            } else {
                9
            },
            Ordering::Relaxed,
        );

        // StablePlayerAi participates in the game's simulation/presimulation machinery. The
        // live match's authoritative AI decisions can therefore arrive through ServerPresim,
        // not only through the presentation-side ClientMatchView/ClientSpectate origins. This
        // mod deliberately bridges client input into that path for single-player direct-control
        // testing; replay/tool/unknown simulations remain blocked.
        if !is_match_view && !is_spectate && !is_server_presim {
            return base_input;
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
