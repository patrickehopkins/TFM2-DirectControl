# Candidate A pacing validation log

This file records physical validation of the staged pacing work for Teamfight Manager 2 v0.5.8.

## Stage 1 — read-only StablePlayerAi observer

Status: **PASS — physically validated 2026-09-10**.

StablePlayerAi callbacks on Candidate A's worker were proven to expose the watched simulation tick. Candidate A raced hundreds of simulated seconds ahead of presentation when unpaced, all ten player ids appeared on the same worker, and callbacks stopped when Candidate A completed.

## Stage 2A — direct `match_view + 0x250` played-tick hypothesis

Status: **FAIL — physically rejected 2026-09-10**.

The value remained exactly `1` at visible 00:02, 00:05, 00:20, and 00:30. It is not the advancing presentation tick.

## Stage 2B — adjacent playback accumulator decode

Status: **FAIL — physically rejected 2026-09-10**.

Common integer/floating-point/Duration interpretations of `match_view + 0x258/+0x260` failed closed rather than matching presentation.

## Stage 2C — ranked match-view clock scan

Status: **FAIL — physically rejected 2026-09-10**.

A read-only scan of `match_view + 0x000 .. +0x95C` found no scalar field whose change tracked the visible clock. Further playback-clock archaeology is deferred because it is not required for real-time pacing.

## Stage 3A — bounded 60 Hz Candidate-A pacing

Status: **PASS — physically validated 2026-09-10**.

Candidate A was paced from its StablePlayerAi callback at 60 ticks per monotonic wall-clock second with ~35 ms allowed lead, while `base_input` remained vanilla AI. Representative results:

```text
visible 00:15 | origin 481 | tick 1416 | elapsed 15547 ms
visible 00:20 | origin 481 | tick 1725 | elapsed 20687 ms
visible 00:25 | origin 481 | tick 2031 | elapsed 25797 ms
```

The temporary 30-second release then let Candidate A race ahead and complete normally. This proved the core pacing technique.

## Stage 3B — continuous 60 Hz full-match pacing

Status: **PASS — physically validated 2026-09-10**.

The artificial 30-second release was removed. Candidate A remained `active 1 / done 0` throughout a ten-minute watched match with no abnormal behavior and `safety fail-open no` throughout.

Representative long-run result at visible 10:00:

```text
origin tick 10 | tick 36039 | elapsed 600438 ms | safety fail-open no
```

Observed simulation delta was `36029` ticks. Exact 60 Hz over 600.438 seconds predicts ~`36026` ticks, so the simulator was only about three ticks (~50 ms) ahead after ten minutes. The fixed worker thread id within the match is expected; different matches use different worker ids.

The user tested the game's normal playback-speed controls during paced simulation. The UI retained its chosen 3x/0.5x/etc. state, but actual presentation remained effectively at 1x while Candidate A was live. That was desirable for the first direct-control baseline; synchronized variable-speed live control is now the subject of Stage 6 below.

## Stage 3C — Ctrl+End irreversible finish release

Status: **PASS — physically validated 2026-09-10**.

Ctrl+End was added as a deliberate one-way release. It sets `manual finish YES`, permanently disables pacing/manual control for that match, and lets Candidate A finish at native full speed. Starting a new match resets the latch.

Physical test:

- normal continuous pacing through roughly visible 00:40;
- Ctrl+End pressed once;
- overlay changed to `manual finish YES` while `safety fail-open no` remained unchanged;
- Candidate A immediately raced from the low-thousands tick range toward the end of the simulation;
- `active 0 / done 1` appeared around visible 00:47-00:48, roughly 6-7 seconds after release, consistent with normal unpaced simulation duration;
- visible replay continued normally.

This confirms the release flag is removing real simulation backpressure rather than merely changing UI state.

## Stage 4A — first real manual MoveTo

Status: **PASS — physically validated 2026-09-10**.

The first gameplay-mutation test proved that manual `InputV1::move_to` commands can be injected into the paced watched simulation in real time. Selected champions visibly changed course toward RMB targets, repeatedly retargeted, retained persistent destinations through death/respawn, and other AI actors responded normally to the commanded champion's changed behavior.

The initial implementation used raw player-slot/player-id assumptions. Runtime testing later proved that visible F-key card order does not match Candidate A's internal `player_id` order, so selection was generalized to all ten visible cards and resolved by athlete identity instead of team or side.

Hostile-unit click interpretation is intentionally **not** part of Stage 4A. The parallel `feat/cursor-entity-picking` branch will later distinguish ground MoveTo from hostile Attack once the basic command path is stable.

## Stage 5A — prematch hard start gate

Status: **FAIL — loading/simulation separation hypothesis rejected 2026-09-10**.

Candidate A was first held inside its earliest observed AI callback so the watched simulation could not advance until Ctrl+Home. Pressing Start Match froze the game on the tactics screen and Windows eventually reported the executable as not responding. Ctrl+Home could not release the hold because the original listener depended on `post_render`, and the blocked Start Match path was no longer pumping render/UI callbacks.

This physically proved that the client cannot complete the Start Match transition while Candidate A is stopped inside that callback. At least some simulation progress is a synchronous dependency of match loading.

## Stage 5B — one-complete-tick gate with fail-safe release

Status: **PARTIAL PASS / separation still rejected — physically validated 2026-09-10**.

The gate was moved until after one complete simulation tick and given two safety mechanisms: Ctrl+Home polling from the Candidate-A worker itself, plus an automatic bounded release into the proven 60 Hz pacer. The game no longer hung and Start Match completed normally.

Runtime diagnostics consistently showed the bounded startup hold expiring at about 1.6 seconds before the battlefield became interactive, after which Candidate A ran under the normal 60 Hz pacer. Example early-match diagnostics included:

```text
visible 00:04 | start YES | origin 2 | elapsed 4750 ms | start held ~1620 ms
visible 00:13 | origin 2 | elapsed 4985 ms | start held ~1620 ms
```

Therefore one complete tick is still insufficient for the battlefield to become independent of simulation progress. The fail-safe behavior works, but a true zero-pre-simulation manual start cannot be implemented by simply blocking StablePlayerAi callbacks this early.

**Decision:** startup gating is shelved for later polish. It is not a blocker for higher-priority command work. See `docs/known-issues.md`.

## Stage 5C — pause/resume gate

Status: **PASS — physically validated 2026-09-10**.

The direct `pause_ui` visibility detector freezes Candidate A while the game's pause menu is open and resumes correctly when the menu closes. Manual commands survive the pause, the selected champion continues toward the previously commanded destination after resume, and a long pause does not become catch-up budget or cause a visible high-speed burst afterward.

Representative diagnostics showed `phase PAUSED` with an increasing `pause held` counter while the visible clock remained fixed, followed by a fresh pacing origin after resume.

## Stage 5D — athlete-aware F-key selection

Status: **PASS — physically validated 2026-09-10**.

Visible F1-F10 cards are now resolved to stable athlete identities rather than assuming any team, side, or internal player-id ordering. Physical testing selected `misutaaa` with F3; diagnostics resolved the visible card to athlete 2, manual-input returns advanced for athlete 2, and the visibly selected/controlled champion was the intended character before and after pause/resume.

This preserves the project's team-neutral design: direct control does not need to know which side belongs to the player.

## Stage 6A — synchronized ordinary match speeds

Status: **IMPLEMENTED / awaiting physical validation**.

The stable client UI already exposes the five ordinary native match-speed selectors. Direct Control now reads the game's selected 0.5x/1x/1.5x/2x/3x state and maps Candidate-A pacing to 30/60/90/120/180 simulation ticks per wall-clock second. A rate change immediately re-anchors the pacer so time accumulated under one rate never becomes catch-up or slow-down budget under another.

This intentionally keeps the game's own presentation controls authoritative. Clicking a native speed button or using a shortcut that changes the native selected speed should therefore change presentation and Candidate-A pacing together without a second Direct Control speed UI.

The debug overlay reports both the interpreted speed label and pacing Hz so physical testing can verify the selected-state bridge directly.

**Multiplayer hard rule:** Direct Control latches multiplayer after seeing the stable management `Room` or `Lobby` scene. While latched, the mod ignores timeline speed-selector state and fixes Candidate-A pacing at 60 Hz / 1x. The native game's own multiplayer speed restrictions remain authoritative; Direct Control does not add or honor any speed-changing behavior there.

Highlight/death fast-forward is deliberately not part of Stage 6A. Highlight is not treated as an ordinary fixed-rate selector and will be implemented only after the five ordinary rates pass together.

## Current merge baseline

Status: **READY FOR COMMAND EXPANSION**.

The physically validated baseline now includes:

- Candidate A identified as the watched-match simulation;
- continuous ~60 Hz pacing for long matches;
- pause and resume without hidden catch-up simulation;
- team-neutral F1-F10 actor selection through athlete identity;
- real-time persistent RMB `MoveTo` injection;
- commands retained correctly across pause/resume;
- one-way Ctrl+End release that lets the simulation finish at native speed;
- fail-open behavior that avoids permanent hangs during experimental startup gating.

No additional pacing/control validation is required before adding new commands. Startup sequencing, cursor-origin calibration, replay-speed integration, and final persistent-command semantics are documented as known/deferred work rather than blockers.

## Known cursor-projection issue

The yellow world-space command marker remains offset from the actual mouse reticle. The error stays approximately constant through zoom changes, which points to an origin/viewport calibration error rather than a zoom-scale error. Current diagnostics report `origin center_log`; the projection still uses the `ingame.center_log` UI node as its reference origin, which is not guaranteed to be the actual center/origin of the `Game` draw-map viewport.

This is not currently blocking movement-control validation, but it must be corrected before precision unit/ground picking is considered reliable.

## Parallel branch note

`feat/cursor-entity-picking` was created from the Stage-1 pacing tip. Its entity-picker concepts remain useful, but the live-control branch has since changed selection and control architecture substantially. Do not merge that branch wholesale. Rebase or transplant the picker logic when actor/entity targeting becomes current work, with manual integration expected in at least `src/lib.rs` and `src/control.rs`.
