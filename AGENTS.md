# Contributor and agent instructions

These instructions apply to the current `main` implementation. For behavior, use `README.md` first; for outstanding defects use `docs/known-issues.md`; for deliberately deferred experiments use `docs/deferred-investigations.md`. Historical plans and validation logs are evidence, **not instructions to reimplement the current mod**.

## Project and supported build

Harbinger Direct Control runs Teamfight Manager 2's watched match near real time and publishes commands for a selected champion through the official stable native Rust SDK. The current Harbinger release target is **v0.1.5** on **TFM2 v0.6.2 Windows/Steam**, single-player first. Do not claim that a new game version, Linux/Steam Deck, replay determinism, or multiplayer works without separate validation.

## Current behavior to preserve

- `Ctrl+Home` works only after the startup synchronization prompt reaches READY; it resumes the paced watched simulation.
- `F1-F5` select the manager's Top/Jungle/Mid/Bottom/Support champions; `F6-F10` select the opposing team in the same lane order. The mapper relates the manager's club ID to athlete contract club IDs to identify the manager's actual blue/red simulation side, then uses Candidate-A team/lane/athlete identities. Club IDs are NOT comparable directly to Candidate-A side IDs; never use simulation player-ID ordering or UI card names. Both teams are controllable; duplicate player names, changed native follow bindings and a hidden HUD do not prevent normal selection.
- RMB issues contextual ground movement or exact-target attacks, including held/swept RMB and minimap requests. `A` then LMB is attack-move; `H` is Hold; `B` is Return.
- Q/W/R use runtime-validated skill targeting. Targeted/position/direction actions use LMB confirmation; genuine cursorless/self-only skills cast immediately. Cooldown/locked actions must not queue delayed casts.
- `End` yields the selected champion back to vanilla AI, **without** releasing paced simulation or native replay-seek suppression. Confirmed `Ctrl+End` globally and irreversibly releases control and pacing for that match.
- While Harbinger owns the live match, native replay seek/highlight actions are suppressed **by semantic action**, including remapped shortcuts. Do not restore these actions on temporary `End` release. Ordinary synchronized pause remains supported.
- MMB drag and mouse-wheel zoom are validated match-wide camera controls. Automatic native team fog applies to the controlled champion, and enforcement stops on `End`.
- The supported live pacing baseline is 1x / approximately 60 Hz. Variable-speed experiments are deferred. Any future speed-affecting feature must be non-functional in multiplayer; multiplayer itself has not been validated.

## Implementation constraints

- Prefer Team Samoyed's **stable native Rust API** for gameplay commands. Do not switch to the deprecated classic SDK.
- Preserve vanilla AI and vanilla automatic shopping when no champion is manually controlled.
- External Windows input crosses a deterministic simulation boundary; keep acquisition separate from simulation-side command generation and retain foreground-focus checks on raw keyboard/mouse paths.
- The stable SDK does not expose every camera, simulation, or native replay-input detail. The version-checked detours in `src/camera_probe/`, `src/simulation_probe.rs`, and `src/replay_action_gate.rs` are intentional and runtime-sensitive. Do not remove or generalize them solely because their names mention probes.
- Preserve exact executable fingerprints, validated hook boundaries, native UI paths, and fail-safe behavior. Recheck signatures and physical behavior for every supported executable change; compilation alone does not establish compatibility.
- Do not modify game saves or ship the game's executable, proprietary assets, local SDK copy, development DLLs, or probe output.

## Change and validation workflow

1. Make focused changes on a branch and submit a pull request. Do not merge functional changes without maintainer approval.
2. Read the applicable current source and validation docs before changing native hooks, pacing, replay suppression, camera, or control semantics.
3. Set up the locally installed game SDK using `scripts/bootstrap-sdk.ps1`; follow `README.md` and `CONTRIBUTING.md` for prerequisites and build steps.
4. Run `cargo fmt --check` and `cargo check`; when practical, run `cargo test` and build a release DLL.
5. For behavior/native changes, perform an actual Windows v0.6.2 match smoke test. Inspect `log.log` on startup or hook failures. Do not relabel an untested change as validated.
6. Update `README.md` and current-status docs when behavior changes. Place unfinished investigations in `docs/deferred-investigations.md` or a clearly marked historical log rather than turning old plans back into active requirements.

AI-assisted contributions are welcome; accuracy and reproducible evidence matter more than whether an AI helped write the patch.
