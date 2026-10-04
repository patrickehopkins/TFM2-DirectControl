# Contributing to Harbinger Direct Control

Thanks for helping improve Harbinger. This repository intentionally retains some early research and diagnostic utilities because native Teamfight Manager 2 integration can change between game updates. Please distinguish **validated shipping behavior** from historical investigation plans.

## Before starting

- Read `README.md` for current controls, supported environments, design overview, and build instructions.
- Read `AGENTS.md` for technical guardrails (applicable to human and AI-assisted contributions).
- Check `docs/known-issues.md` for known limitations and `docs/deferred-investigations.md` for proposed features, prior failed approaches, and investigation branches.
- `docs/release-week-plan.md` and `docs/release-scope.md` preserve **historical** planning decisions. They are not current implementation instructions. `docs/keybind-plan.md` records **future proposals**, not existing settings or accepted replay behavior.

## Local development

The tested target is **Teamfight Manager 2 v0.6.2 on Windows/Steam**. The release is single-player first; neither multiplayer compatibility nor other operating systems should be assumed from a successful build.

1. Install the game and a Rust toolchain. Open PowerShell in this repository's root.
2. Run `./scripts/bootstrap-sdk.ps1` (PowerShell also accepts `.\scripts\bootstrap-sdk.ps1`). If your game is in a different Steam library, pass `-GameDir "YOUR_GAME_INSTALL_FOLDER"`.
3. Run `cargo fmt --check` and `cargo check`. Build with `cargo build --release`.
4. To build **and** install into your local game, run `.\scripts\install-dev.ps1`, with `-GameDir` when needed. This script refreshes the local SDK, builds, and copies the DLL and metadata. It is intended for development and should not be treated as a Workshop release artifact.

The SDK is copied locally into the Git-ignored `sdk/` directory. Do not commit the game SDK, game executable, generated `target/` output, `dist/` packages, local logs, or temporary diagnostic reports.

Python tools in `tools/` are optional, specialized reverse-engineering and compatibility aids. They are not required for an ordinary Rust build, and successful static verification is **not** a substitute for live testing.

## Non-negotiable behavior

- F1–F5 address the manager's five champions in Top/Jungle/Mid/Bottom/Support order and F6–F10 the opposing team in the same order. Resolve those slots from the manager-team ID plus Candidate-A athlete/team/lane identity, not player names, visible UI card text, game follow bindings or simulation player-ID arithmetic. Both teams remain controllable.
- `End` temporarily yields the champion to normal AI without releasing the live pacer **or** replay-seek suppression.
- Confirmed `Ctrl+End` is the irreversible global release for that match. Native seeking/highlighting returns only after that release.
- Seek/highlight suppression must work even when users rebind native shortcuts. Read `docs/replay-native-action-analysis.md` before changing the version-specific native gate.
- Maintain the validated camera and input-focus safeguards; do not synthesize native keypresses or guess new native offsets without evidence.
- Harbinger's live-pacing baseline is 1x (approximately 60 Hz). Experimental variable-speed work is deferred; any future multiplayer implementation must disable every mod-provided speed-affecting feature and keep multiplayer at 1x.

## Pull requests and testing

Keep each PR focused, explain user-visible behavior and any relevant assumptions, and link supporting logs or steps to reproduce problems. For changes affecting native hooks, pacing, input, camera, targeting, or replay behavior, identify the exact tested game build and describe an actual match-level smoke test. For documentation-only changes, identify the authoritative source used and preserve the approved Workshop promotional copy unless changing it was specifically requested.

Useful checks from the repository root:

```powershell
cargo fmt --check
cargo check
cargo test
cargo build --release
```

Run the tests that apply to your change. If you cannot physically test a runtime change, say so plainly in the PR rather than marking it validated. Maintainer approval is required before merging functional changes.

## Status, scope, and documentation

Current behavior belongs in `README.md` and its linked architecture documents. Confirmed release limitations belong in `docs/known-issues.md`; deferred work and useful investigation recovery points belong in `docs/deferred-investigations.md`. Label experiments and superseded hypotheses as historical. Never copy a proposed feature from an old plan into current-status documentation without validating it.

The owner has not yet selected a repository-wide open-source license. Public visibility, by itself, does not grant permission to distribute or relicense the project's code or third-party/game components. Confirm the eventual license before planning downstream redistribution.
