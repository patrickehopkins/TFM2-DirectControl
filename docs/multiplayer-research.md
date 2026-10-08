# Multiplayer reconnaissance plan

Status: **future research plan; no multiplayer compatibility claim.**

## Goal

Determine whether direct manual control can be made deterministic and synchronized across TFM2 multiplayer peers **before** designing any custom netcode.

Do not begin with rollback, state snapshots, reconciliation, or a user-facing multiplayer mode. First establish what TFM2 already synchronizes and where Harbinger's external human input crosses that boundary.

## Core questions

1. Do both multiplayer clients run equivalent Candidate-A/watch-match simulations?
2. Do corresponding simulation ticks match?
3. Do entity IDs, teams, positions, HP, cooldowns, inventory, and other stable state match at the same tick?
4. Is one peer/host authoritative, or are both clients running deterministic peers?
5. What native events/packets occur at match start, pause, manager interactions, and any other synchronized controls?
6. Can both peers observe the same vanilla AI InputV1 decisions?
7. If one client injects a harmless manual InputV1 and the other does not, what happens?
8. Does TFM2 replicate the input, reconcile state, detect a desync, or simply diverge?

## First milestone: observe only

Instrument both clients without enabling manual control.

Record at regular deterministic checkpoints:

- simulation tick;
- stable athlete/entity identity;
- selected representative entity state;
- controlled-team/player state where available;
- a compact deterministic state hash.

The exact hash schema should begin small and stable. Avoid hashing presentation-only state.

Example conceptual record:

```text
tick=12600
entity=player_3
pos=(...)
hp=...
cooldowns=(...)
state_hash=...
```

The purpose is to prove whether two ordinary multiplayer clients remain equivalent under vanilla play.

## Second milestone: one harmless divergent input

Only after ordinary state equivalence is demonstrated:

- choose one clearly identified champion;
- at a known tick, inject one obvious but low-risk `MoveTo(X,Y)` on **one client only**;
- do not inject the same command on the other peer;
- record both clients at high detail around that tick.

Possible outcomes:

### Best case

The other peer reflects the movement without Harbinger transmitting anything.

Implication: a native synchronized player-input/action path may already exist and multiplayer Direct Control could be much easier than expected.

### Middle case

Only the injecting peer moves and simulations diverge.

Implication: Harbinger may need its own synchronized external-input transport. Continue only after measuring how deterministic both peers are absent that external input.

### Authoritative/reconciliation case

The injecting peer is corrected, rejected, or the other side's state wins.

Implication: identify the host/server authority path before designing Harbinger networking.

### Hard failure

The game detects/desyncs/terminates or simulation identity differs unpredictably.

Implication: stop feature work and investigate determinism/authority first.

## If Harbinger must transport inputs

The first prototype should synchronize **commands**, not complete game state.

Conceptual packet:

```text
match/session id
simulation tick
controlled stable identity
command kind
command payload
sequence number
```

Example:

```text
tick=12540
slot=F3 / stable athlete identity
MoveTo(682311,417992)
```

Both peers queue the same external command for the same authoritative simulation tick.

Do not initially send render positions, camera state, UI state, or replay/presentation state.

## Determinism checks

Add periodic state hashes. If peers disagree:

- stop treating the prototype as working;
- preserve the input stream and both state traces;
- identify the first divergent tick;
- determine whether divergence came from command timing, nondeterministic game state, local-only data, or presentation/simulation confusion.

Only after the source of divergence is understood should rollback or snapshot correction even be considered.

## Input rate

Do not assume held RMB requires 60 network messages per second.

A future transport can represent changes in intent:

- start/update movement target;
- start/update exact target;
- attack-move destination;
- skill cast intent/confirmation;
- Hold;
- Return;
- selection/ownership changes.

The simulation can continue emitting the active command locally between changes if deterministic behavior is proven.

## Multiplayer invariants

Until research disproves the need:

- lock multiplayer to 1x / 60 Hz;
- disable experimental speed changes;
- do not expose replay seeking while live control is owned;
- treat presentation synchronization separately from deterministic simulation synchronization;
- use stable athlete/entity identity, never presentation card text or display names.

## Long-term architecture opportunity

If this succeeds, do not build the result as a Control-specific or Harbinger-only one-off.

The desirable reusable layer is conceptually:

> **harbinger-net: deterministic synchronization of external TFM2 simulation inputs**

A downstream mod should be able to provide its local manual command stream while Harbinger infrastructure handles:

- tick agreement;
- stable identity;
- command serialization;
- peer delivery;
- ordering/deduplication;
- state-hash diagnostics;
- desync reporting.

That would let future mods concentrate on gameplay/UI rather than reinventing multiplayer synchronization.

## Networking research, operating budget, and external references (2026-10-08)

**Research only.** Multiplayer Direct Control is not shipped. No transport, networking service, netcode model, or external engine has been selected. Keep this plan distinct from the working single-player mod.

### Hard cost constraint

**$0.00/month** to maintain multiplayer networking. Do not require a rented server, subscription, metered relay, paid matchmaking, or recurring domain registration. Reasonably priced **one-time licenses** may be investigated case by case; none is approved. Prefer player-hosted sessions and modular networking with a direct/alternate connection path if a free service changes policy. Free third-party services are subject to terms, limits, and future pricing changes.

Do **not** assume any particular home ISP. Evaluate ordinary NAT, CGNAT, firewalls, and unfavorable NAT combinations, but do not mistake a Starlink-at-work connection for the developer's personal networking setup.

### Separate feasibility gates

1. **Simulation/control gate (primary unknown):** Observe two *vanilla* TFM2 clients in the same match; confirm whether they share deterministic ticks and game state and whether either is authoritative. Determine whether an external InputV1 command can enter the *live authoritative simulation* at a known tick without violating StablePlayerAi determinism. A precomputed replay or a locally altered presentation is not proof of multiplayer interaction.
2. **Connectivity gate (downstream):** After input injection is shown viable, deliver authenticated commands between machines, with ownership, ticks, ordering, delivery, deduplication, and disconnect behavior. Free transports exist; they cannot repair a missing game simulation hook.

Failure through the **stable mod API** does *not* prove TFM2 multiplayer impossible. Potential later investigations include a supported queued-input interface, upstream mod API request, or explicitly version-scoped native hooks. OS keyboard injection is not a demonstrated solution to independently controlling 2–10 champions. Do not implement a large network stack before this gate is solved.

### Candidate connectivity paths — none adopted

| Approach | Possible role | Important qualification |
| --- | --- | --- |
| Loopback / LAN transport | Local two-process and two-computer probes | Does not establish public internet reachability |
| Direct UDP / WebRTC / ICE | Player-hosted peers without rented servers | Requires signaling, security, NAT traversal, and possibly relay fallback |
| Epic Online Services P2P / lobbies / relay | Potential no-hosting-fee connectivity | Verify **third-party TFM2 mod** eligibility, SDK terms and credentials; cannot assume permission to use TFM2's product identity |
| Steamworks P2P / Steam Datagram Relay | Possible Steam-based connections | Owning/modding a Steam game does not confer Steamworks app credentials or guaranteed access to Valve's relays. Open-source GameNetworkingSockets does not bundle the proprietary Steam relay backend |
| Cloudflare Tunnel + WebSockets | HTTPS/WebSocket signaling or experimental command relay | Not a free generic UDP game relay. Named production hostnames generally need a domain; Quick Tunnels are for development. Paid Spectrum is excluded |
| Tailscale / ZeroTier | Closed collaboration/testing | Free-tier user/device restrictions and possible accounts make them unproven as a public ten-player infrastructure solution |

All service terms and free-tier limits must be rechecked before any adoption. A third-party relay is never a guarantee of universal free connectivity.

### Prototype design and test sequence

- Preserve the existing two-client observation, identical-tick hash, and one-sided harmless MoveTo experiment above. Compare inputs and simulation state *before* selecting lockstep, host authority, snapshots, or rollback.
- If injection works, isolate: network transport -> authorized match/participant ownership -> ordered tick-indexed command queue -> deterministic simulation. **Never read sockets within deterministic callbacks.**
- Exchange command intent and stable entity identity, not camera/render state; include session, simulation tick, sequence and integrity checks.
- Establish two participants first. **Up to ten human-controlled champions** is a later aspiration, not a verified capability.
- Exercise loss, delay, jitter, packet reordering, firewalls/NAT, disconnects, repeated joins, and first divergent tick with existing emulation tools before building a custom test laboratory.
- Keep player-facing claims at "single-player only" until actual multiplayer passes repeatable two-machine tests.

### Reference hierarchy

**Established conceptual guidance** — Gaffer on Games (https://gafferongames.com/categories/networked-physics/), Valve Source multiplayer architecture (https://developer.valvesoftware.com/wiki/Source_Multiplayer_Networking), GGPO's deterministic save/restore/rollback requirements (https://github.com/pond3r/ggpo/blob/master/doc/DeveloperGuide.md), and GameNetworkingSockets' transport-versus-service distinction (https://github.com/ValveSoftware/GameNetworkingSockets/blob/master/README_P2P.md). **TFM2's actual API and peer-level probe results override any generic architectural inference.**

**Secondary implementations to study, not adopt by mention** — GGRS (https://github.com/gschup/ggrs) for sync tests and multiple participants; Drift Engine (https://driftengine.dev/) for a compact example of deterministic stepping, rewinds, and simulated poor network conditions. Drift is neither a new networking paradigm nor established proof of large-scale reliability, and **there is no commitment to use it**.

**Provider policy sources** — EOS licensing (https://onlineservices.epicgames.com/licensing) and acceptable use (https://onlineservices.epicgames.com/services/terms/aup); Steam networking (https://partner.steamgames.com/doc/features/multiplayer/networking); Cloudflare Tunnel (https://developers.cloudflare.com/tunnel/get-started/) and Spectrum (https://developers.cloudflare.com/spectrum/get-started/).

## Non-goals for the reconnaissance phase

- rollback;
- prediction;
- latency compensation;
- state snapshots;
- matchmaking;
- NAT traversal;
- anti-cheat;
- spectator networking;
- polished multiplayer UI.

Those are all downstream questions. First establish TFM2's native synchronization model and whether identical externally supplied InputV1 commands remain deterministic.
