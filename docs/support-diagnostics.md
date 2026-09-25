# Harbinger v0.1.2: automatic support diagnostics

The public maintenance build records startup, keyboard activation and champion mapping
diagnostics **automatically** while the player uses Harbinger normally. There is no
debug hotkey, console command, special launch option or additional test procedure.

## What to request from a player

1. Let Steam update Harbinger and restart Teamfight Manager 2.
2. Play a match and try Ctrl+Home, then F1 to select a champion, as usual.
3. If it still fails, send the single file below after closing the game:

`%APPDATA%\TeamSamoyed\TeamfightManager2\data\harbinger-diagnostics.log`

Players can paste that location directly into Windows Explorer's address bar.
The file's path is also printed in the game's ordinary `log.log` during mod
initialization, allowing us to check whether automatic diagnostics started.
If the companion log does not exist, request the adjacent `log.log` instead.

No private keyboard activity is logged: we only record Harbinger's Ctrl+Home
and F1-F10 input attempts when the game owns foreground focus. We do not log
raw text input. The support log is append-only with a roughly 1 MiB
startup-rotation policy (one previous log retained); the latest session is
separated by an explicit process-start header.

## What the maintainer will see

- Game version and guarded hook initialization errors.
- Match lifecycle and phase changes.
- Every 15 seconds in a visible match: phase, start state, synchronization,
  readiness tick, visible clock, last watched simulation tick, foreground
  process ownership, replay safety, native speed-override state, and whether a
  champion is selected.
- Ctrl+Home presses: detected and either activated or still blocked. The worker
  path also logs Ctrl+Home presses if a loading callback is blocking rendering.
- An automatic ten-slot roster-health report once manual control is enabled.
- F1-F10 presses and the corresponding selection success or precise mapping
  failure (no visible card, unmatched athlete, ambiguous duplicate, etc.).

## Safe scope of this patch

A malformed or missing card no longer prevents selection of every other valid,
non-ambiguous card. Ambiguous or duplicate athlete assignments still fail
closed to avoid controlling the wrong champion.

The validated live simulation, 1x pacing, native replay safety gate, pause,
skill commands, camera hooks, and multiplayer-disabled behavior are unchanged.
If the current public build is reliable locally, do not merge or publish
until this branch passes a Windows build and the smoke tests below.

## Maintainer's release acceptance checks

- Fresh Windows/Steam v0.6.1 Workshop-equivalent installation.
- Ordinary match startup, wait for READY, Ctrl+Home, then F1-F10 selection
  for all ten visible cards on an unchanged English-language game.
- Ordinary movement, attacks, abilities, pause, End, Ctrl+End, and a new match.
- Repeat the activation/selection test with the same other-mod combinations
  previously checked by the maintainer.
- Confirm the support log appears without any special action, has bounded
  event frequency, and records Ctrl+Home, mapping health and selection outcomes.
- Temporarily rebind a native F-key follow shortcut and verify any altered UI
  card text is diagnosed rather than selecting the wrong actor.
- Confirm `log.log` shows the companion-file path. If creation is blocked,
  verify that error is recorded in `log.log`.

Publish as an **update of the same Workshop item**, preserving
`mod.workshop_id`. Do not rewrite the hand-edited Steam description during
diagnostic maintenance.
