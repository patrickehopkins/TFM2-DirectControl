# Manual-control validation log

> **Historical staged-validation record.** This file captures the September 10 path by which live manual input was first proved. Its Stage 4/5 F-key/card mapping is **not the current selector architecture**. Harbinger v0.1.5 on TFM2 v0.6.2 uses manager club identity + athlete contract club ownership + Candidate-A team/lane/athlete identity: F1-F5 select the manager's Top/Jungle/Mid/Bottom/Support and F6-F10 the opposing team. Current selection does not depend on visible card text, card visibility, player names, native follow bindings, or raw simulation player-ID order. See `README.md` and `docs/core-control-contract.md`.


## Stage 4A — first paced RMB MoveTo

Status: **FAIL — physically rejected 2026-09-10**.

Observed behavior:

- simulation pacing remained functional;
- F6/F7-based selection produced no visible champion response to RMB clicks;
- the expected cursor/world marker was not observed during the test.

Two implementation problems were identified before requesting another physical test:

1. The client projection path treated `ingame.center_log` as a required viewport gate. If that UI node was unavailable or did not describe the current battlefield region, projection returned `None`, which prevented both the marker and RMB target publication.
2. The published `InputV1::move_to` destination omitted the required conversion from camera/world coordinates to simulation fixed-point coordinates. Stable simulation positions use 1000 simulation units per camera/world unit, so a world point such as `(800, 350)` must be published approximately as `(800000, 350000)`, not `(800, 350)`.

Stage 4A also encoded an unnecessary F6-F10/"player team" assumption. That policy has been removed.

## Stage 4B — team-neutral projection + corrected simulation scale

Status: **PASS — physically validated 2026-09-10**.

Changes and physical results:

- F1-F10 were generalized to all ten visible match cards with no team/ownership restriction;
- cursor projection no longer depended on `ingame.center_log` as a hard viewport gate;
- camera/world coordinates were multiplied by 1000 before becoming simulation coordinates;
- RMB publication and manual `InputV1::move_to` returns were observed on Candidate A;
- commanded champions visibly changed course and repeatedly retargeted in real time;
- other AI actors reacted naturally to the changed champion behavior;
- a persistent MoveTo could survive death/respawn and cause the champion to return to the stored destination.

This physically proved the live command bridge.

## Stage 5 — stable actor identity

Status: **PASS — physically validated 2026-09-10**.

Further testing found that visible F-key card order is **not** Candidate A's raw `player_id` order. A direct arithmetic mapping could select the wrong champion; for example, an F3 UI selection initially caused a different actor to receive commands.

The selector was changed to resolve the displayed card to stable athlete identity, then match `StableAiContext::athlete_id()` on Candidate A. Physical retesting selected `misutaaa` with F3, resolved the intended athlete, and moved the correct champion before and after pause/resume.

**Historical Stage 5 behavior:** at this point in development, F1-F10 were calibrated from visible cards and the primitive was deliberately team-neutral. **This mapping was later superseded.** Current v0.1.5 semantics are F1-F5 = manager team and F6-F10 = opponent, each ordered Top/Jungle/Mid/Bottom/Support, with stable athlete identity and contract-club ownership used to resolve the manager's actual simulation side. Both teams remain controllable; the manager/opponent split defines shortcut ordering, not a permission boundary.

## Pause/resume command behavior

Status: **PASS — physically validated 2026-09-10**.

The direct `pause_ui` gate freezes Candidate A while paused. Closing the pause menu re-anchors the 60 Hz pacer and resumes Candidate A without hidden catch-up simulation. Persistent movement state survives the pause, so the selected champion continues the existing MoveTo after resume and accepts new RMB commands normally.

## Historical cursor calibration issue — later cleared

At this stage, the yellow projected world marker was consistently displaced from the physical mouse reticle. That was an intermediate calibration defect, not a current release limitation. Later camera/projection and contextual-targeting work physically validated the corrected transform and click targeting. See `docs/camera-controls.md`, `docs/camera-validation-log.md`, and `docs/click-targeting-geometry.md` for the later state.

## Command-expansion readiness

The movement/control foundation is ready for additional commands. No further generic MoveTo/pacing test is required before implementing attack, cast, return-to-base, or related command primitives. Each new command should receive its own narrow physical validation rather than reopening the already-proven simulation/pacing architecture.
