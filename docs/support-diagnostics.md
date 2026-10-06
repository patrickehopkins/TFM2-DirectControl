# Harbinger v0.1.5: automatic support diagnostics

Automatic support diagnostics were introduced in v0.1.2 and remain enabled in
normal v0.1.5 gameplay. They record startup, keyboard activation, authoritative
ten-champion roster mapping, and selection outcomes without a debug switch.

Each compiled DLL embeds a support build stamp (Git revision, dirty marker when
available, and build timestamp). This distinguishes development builds sharing
a public version; it is **not** a cryptographic hash of the DLL.

## What to request from a player

1. Let Steam update Harbinger and restart Teamfight Manager 2.
2. Start a match, wait for READY, press Ctrl+Home, and try the affected F-key.
3. If control fails, close the game and send this single file:

`%APPDATA%\TeamSamoyed\TeamfightManager2\data\harbinger-diagnostics.log`

Players can paste that path directly into Windows Explorer's address bar.
The companion log's path and build identifier are also printed in the ordinary
game `log.log` at mod initialization. If the companion log cannot be created,
request `log.log` instead.

Harbinger logs its Ctrl+Home and F1-F10 attempts only when the game has foreground
focus; it does not record raw text entry. The companion log is append-only with
roughly 1 MiB startup rotation and one previous log retained. Each process
start has a separate header.

## What the maintainer will see

- Game version, compiled build identifier, and guarded hook initialization errors.
- Match lifecycle and phase changes.
- Every 15 seconds in a visible match: phase, start state, synchronization,
  readiness tick, visible clock, last watched simulation tick, foreground
  process ownership, replay safety, native speed-override state, and whether
  a champion is selected.
- Ctrl+Home attempts, including whether the request started immediately or was
  queued as `pending`. The worker path also records presses observed during a
  render-blocking loading callback. A queued start should later emit
  `Queued Ctrl+Home activated after interactive + presentation-sync prerequisites`.
- A one-time ten-slot roster-health report once manual control is enabled:
  `candidate_a_roster` count, authoritative roster completeness, resolved
  `team_blocks`, `manager_club_id`, contract identity count, per-side ownership
  evidence, and the resolved club-to-match-side mapping. A full authoritative
  ten-athlete roster is required, but the club relation accepts incomplete
  contract data when each side has at least two matching, noncontradictory
  contract-club records. Missing or conflicting evidence remains visible in
  the one-time roster report.
- F1-F10 attempts, selection successes (stable athlete IDs), and specific
  mapping failures when an authoritative slot or safe fallback is unavailable.

## Selection identity and fallback

Normal v0.1.5 selection uses the manager's persistent club ID, each athlete's contract club ID,
and Candidate A's authoritative athlete/match-side/lane observations. F1-F5 map to the manager's team and F6-F10 to
the opposing team, each in Top/Jungle/Mid/Bottom/Support order. Contract
membership determines the manager's blue/red side; do not compare persistent
club IDs directly to simulation-side IDs. Selection does
not depend on athlete names, visible player-card labels, native follow shortcut
bindings, or whether the match UI is hidden.

If the authoritative roster or manager-team relationship is incomplete, the
mapper can still use unambiguous legacy UI card evidence as a limited fallback.
It rejects ambiguous and duplicate athlete assignments rather than silently
controlling the wrong champion. A failure involving missing `(F1)`-style text
is therefore relevant only to the legacy fallback, **not** an expected failure
with a complete authoritative roster.

The validated 1x/60 Hz simulation, native replay safety gate, synchronized
pause, skill commands, camera hooks, and single-player scope are unchanged by
the v0.1.4–v0.1.5 selection fixes.

## Maintainer's release acceptance checks

- Install the intended Windows/Steam TFM2 v0.6.2 build, with the exact
  Harbinger v0.1.5 release DLL and metadata.
- Verify READY, Ctrl+Home, and all ten F1-F10 selections across both teams.
- Test the manager on **both blue and red**; F1-F5 must always select the
  manager's champions, and F6-F10 must always select the opponents. Also test
  duplicate names on opposing teams, remapped native follow shortcuts,
  and a fully hidden match UI. The maintainer reported the original three
  selection regressions passing and subsequently confirmed the red-side
  club-ownership fix in the revised development build. Repeat the essential
  checks against the exact final Workshop package.
- Verify movement, attacks, skills, pause/resume, End, Ctrl+End, and a fresh
  match on the final packaged build.
- Verify that the companion log identifies the compiled build and reports a
  complete 10/10 Candidate-A roster and both team blocks.
- Check `log.log` if companion logging or native hook installation fails.
- Keep the original `mod.workshop_id` and update the existing Workshop item
  rather than creating a duplicate listing.

A startup-stall watchdog is not part of this maintenance update; existing
match lifecycle and heartbeat diagnostics remain the first investigation path.
Do not rewrite the maintainer's hand-edited Steam Workshop description as
part of diagnostic maintenance.


## Ctrl+Home queued-start hardening (development after v0.1.5)

A startup race was identified in the control handoff itself, independent of F-key
selection. The render thread could observe the Ctrl+Home rising edge immediately
before the same frame/update published either `INTERACTIVE_MATCH=true` or
`STARTUP_PRESENTATION_SYNCED=true`.

Previously, `request_start_simulation()` simply returned when synchronization
was not ready. The physical key edge had already been consumed, so the user had
to release and press Ctrl+Home again. Depending on timing, that could look like
the mod was permanently stuck in spectator mode.

The hardened design separates **user intent** from **safe activation**:

1. a valid foreground Ctrl+Home press records `pending_start=true`;
2. Harbinger continues to hold Candidate A;
3. once the verified replay gate, interactive match, and presentation sync are
   all true, the pending request activates automatically;
4. the pending flag is cleared only on activation or match reset.

This is intentionally a latch, not a relaxation of startup safety. Ctrl+Home
still cannot start an unsupported replay-gate build or bypass presentation
synchronization.
