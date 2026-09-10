# Manual-control validation log

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

The control primitive remains intentionally team-neutral. F1-F10 mean "the athlete shown on this visible card," not "my team" and not a raw internal player id.

## Pause/resume command behavior

Status: **PASS — physically validated 2026-09-10**.

The direct `pause_ui` gate freezes Candidate A while paused. Closing the pause menu re-anchors the 60 Hz pacer and resumes Candidate A without hidden catch-up simulation. Persistent movement state survives the pause, so the selected champion continues the existing MoveTo after resume and accepts new RMB commands normally.

## Known cursor calibration issue

The yellow projected world marker remains consistently displaced from the physical mouse reticle. The offset is approximately stable across zoom levels, indicating that world scale is correct but the screen/viewport origin is not yet calibrated correctly.

This does **not** invalidate coarse ground MoveTo, which is physically proven, but precision actor/entity selection should not rely on the current transform until the origin is corrected. See `docs/known-issues.md`.

## Command-expansion readiness

The movement/control foundation is ready for additional commands. No further generic MoveTo/pacing test is required before implementing attack, cast, return-to-base, or related command primitives. Each new command should receive its own narrow physical validation rather than reopening the already-proven simulation/pacing architecture.
