# v0.6.1 native replay action suppression

**Implementation status: physically smoke-tested on the supported v0.6.1 build (user-reported pass, 2026-09-23).** The Windows development build installed and the replay protection, control, pause, tooltip, speed, temporary-release, global-release, and next-match smoke tests passed. This validates the tested build only; recheck native signatures and repeat runtime tests after executable changes.

## Executable identity and verified runtime path

All findings below come from offline disassembly of the user-supplied TeamfightManager2.exe:

- Size: 86,330,880 bytes
- SHA-256: `91084e9a29c70993595a1d7d0c22064bae0ee82b0fc773c696077d15c2268f98`
- PE timestamp: `0x6AB1D950`; PE image size: `0x05264000`
- All addresses below are **relative virtual addresses** (RVA) for this exact build. No guessing other versions.

TFM2's live-match camera/input handler at RVA `0x00C2DBE0` calls the native **current action-to-binding getter** at `0x021C4CE0` with a semantic action ID and then compares its returned key with the incoming input event before executing the respective action. Unlike action-name formatting and default-binding registration, this is a runtime dispatch path and looks up the currently configured binding, including user remappings.

| Native action ID | Action | Runtime getter call RVA | Action operation RVA |
| --- | --- | --- | --- |
| `0x1B` | Highlight playback mode | `0x00C2EC07`, `0x00C2ED1E` | match-handler branches |
| `0x30` | Previous Highlight | `0x00C2F4BD` | `0x00C30E90` |
| `0x31` | Back 10 Seconds | `0x00C2F51F` | `0x00C32290` |
| `0x32` | Native timeline Pause | `0x00C2F581` | `0x01D729F0` |
| `0x33` | Forward 10 Seconds | `0x00C2F5C6` | `0x00C31D60` |
| `0x34` | Next Highlight | `0x00C2F628` | `0x00C308D0` |

The getter's complete 12-byte prologue is `56 53 48 83 EC 28 89 D3 88 54 24 27`. Its observed native calling convention is `fn(bindings_ptr, action_id: u32) -> u8` on Win64; only the low byte of the ID is read. The 12 copied bytes are entire instructions and do not reference RIP or preexisting RAX. The default key-map bytes checked for this build do not include `0xFF` as a key.

`tools/verify_replay_action_gate.py` checks the *full executable hash*, PE profile, both existing camera and new getter prologues, semantic getter calls, their immediate input-key comparisons, subsequent replay operations, and the chosen unbound sentinel. It passed against the uploaded executable in offline analysis. **The static verifier alone does not establish runtime stability; the subsequent user-reported Windows/in-game smoke test passed for this build.**

## Staged native gate and lifecycle

`src/replay_action_gate.rs` installs a guarded detour of the runtime binding getter at module initialization. While Direct Control owns the live paced match, the hook returns `0xFF` for `0x1B` and `0x30..0x34`, causing the game's native incoming-key comparisons to fail independent of customized keyboard mappings. For all other actions (including native `0x35/0x36` zoom), it forwards the original lookup through a trampoline unchanged. This does **not** edit stored shortcuts or install a physical-key blacklist.

The safety gate begins when the InGame live session is owned, including before Ctrl+Home and during temporary End spectator yield. It stops as soon as the **confirmed Ctrl+End** release is registered; normal native replay keyboard bindings are then forwarded again. Native replay toolbar buttons and bottom Highlight playback UI are independently hidden through exact stable UI node paths and restored on global release. Stale native Zoom In/Out tooltip suppression is a separate UI cleanup. Native timeline Pause is blocked under this gate; ordinary synchronized pause-menu handling remains intact, but it requires a physical regression test.

If the exact build or getter prologue doesn't match, the hook refuses installation, emits an error to log.log, and **Ctrl+Home will not start Direct Control**. No speculative fallback key blacklist is enabled. The camera hook and native pacing architecture are untouched by this replay gate. It is single-player release scope; the existing locked multiplayer 1x rule is not altered.

### Previous investigation and rejected approaches

Action-name formatter RVA `0x0215D0C0`, default binding registration, and the generic hasher RVA `0x00BA3A30` are **not** runtime replay action rejection points. A temporary feature-gated `replay-native-trace` generic-hasher probe remains available for forensic fallback, but is disabled for standard builds and must not be included in Workshop packaging. Binding-key filters for M/6/etc. are intentionally rejected.

## Physical acceptance test (passed for tested v0.6.1 build; rerun on updates)

On the exact supported v0.6.1 game, close the game and install a **standard** development build (no `-ReplayTrace`):

```powershell
git pull
py .\tools\verify_replay_action_gate.py
.\scripts\install-dev.ps1
```

1. Start a disposable/practice match and start Direct Control with Ctrl+Home. Confirm regular champion control still works and the live hitbox outlines stay aligned with the entities.
2. Test user-remapped `M` (Back 10 Seconds), `6` (Previous Highlight), plus the bindings for Forward 10, Next Highlight, and Highlight playback. **None may change replay position or playback mode.** Also test one *newly rebound* shortcut if practical.
3. Confirm the native replay controls remain hidden/inert, ghost Zoom tooltip no longer appears, and unrelated player-detail UI, MMB drag, mouse-wheel zoom, and the ordinary synchronized pause menu still work. Native timeline Pause shortcut should be inert during ownership.
4. Press **End** to return the champion to AI/spectator while Candidate A remains live-paced; replay shortcuts **must remain blocked**.
5. Confirm **Ctrl+End**. Replay toolbar and ordinary rebound seek/highlight shortcuts must return and operate normally. Starting a new match should re-enable the blocking gate.

**2026-09-23 result:** User reports a clean pass on the full checklist: remapped `M`/`6` and remaining replay keys inactive during ownership; replay controls and Highlight speed hidden; zoom ghost tooltips absent; ordinary controls, tooltips, pause, and available speed controls behave without observed desync; `End` retains protection; confirmed `Ctrl+End` restores ordinary replay; protection re-arms next match. No runtime defect was reproduced in this pass. If a later build fails, capture installer output or log.log and reopen the release blocker.

## Separate deferred hardening

The empirical click-ring/replay alignment diagnostic remains documented in the README. A precise live-vs-presentation playback clock watchdog and automatic snap-to-live are deferred; this release intentionally prevents the primary player-driven cause (native replay seeking), not every conceivable presentation divergence.
