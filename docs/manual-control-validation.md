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

Status: **implemented; awaiting physical validation**.

Changes:

- F1-F10 now map symmetrically to raw player slots 0-9;
- no human-team/ownership inference is performed;
- cursor projection no longer depends on `ingame.center_log`;
- the logical UI cursor is mapped to the full `Game` draw surface and then through the confirmed live camera;
- projected camera/world coordinates are multiplied by 1000 before becoming `InputV1` simulation coordinates;
- the overlay prints both representations as `CURSOR: world (...) -> sim (...)`;
- RMB publication and manual-return counters remain visible;
- Ctrl+End continues to irreversibly release pacing and manual control for the current match.

The Stage-4B physical test should first verify that the world marker follows the mouse and that `CURSOR` reports plausible simulation coordinates. After selecting any slot with F1-F10, RMB should increment the command counter; the selected player's Candidate-A callback should then increment the manual-return counter and visibly obey the MoveTo destination.
