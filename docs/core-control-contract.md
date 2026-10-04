# Direct-control core contract

The direct-control layer is intentionally a low-policy primitive rather than a player-team feature.

## Actor addressing

Teamfight Manager 2 exposes ten visible match cards labeled F1-F10, but runtime testing proved that this visible order is **not** the same as Candidate A's internal `player_id` ordering. The core therefore must not define `F3 == player_id 2` or similar arithmetic mappings.

In the validated v0.1.4 selection layer (merged PR #27), physical F1-F10 remain Harbinger's independent selection shortcuts, even if the game's native follow shortcuts have been rebound:

```text
Candidate A: stable athlete_id + team + lane for each of ten players
management SDK: stable manager-team ID
F1-F5 -> manager's team, by top/jungle/mid/bottom/support
F6-F10 -> other simulation team, same lane order
selected slot -> stable athlete_id -> Candidate A
```

The manager-team ID identifies the **card group**, not a permission boundary. Athletes on both sides remain selectable. If a complete authoritative roster or matching manager-team identity is unavailable, an unambiguous legacy card may still calibrate a block; missing evidence must never cause a guessed identity.

Player names, shortcut labels and card visibility are not inputs to the authoritative mapping. The team/role mapping, duplicate-name independence, remapped-follow-key independence, and hidden-UI behavior passed a live 0.6.2 regression test before PR #27 merged.

The core does **not** reject an athlete because of team ownership. Team/ownership restrictions, if desired, belong to a higher-level mod or feature built on top of this control layer.

This is deliberate. The project should remain useful as a dependency/foundation for future mods that may want to control either side, spectate/control arbitrary actors, build debugging tools, implement alternate game modes, or impose their own permissions.

## Primitive responsibilities

The base layer should provide mechanisms such as:

- keep the watched simulation live near wall-clock time;
- pause/resume that simulation with presentation without hidden catch-up;
- identify/address a live actor without assuming team ownership or UI-order == internal-id order;
- publish movement/action/skill inputs for that actor;
- expose cursor-to-simulation coordinates;
- expose generic entity selection data;
- irreversibly release live control and let the simulation finish.

It should avoid policy such as:

- using manager-team identity to impose team ownership restrictions (it is used here only to establish card-slot order);
- deciding which side may be controlled;
- enforcing game-mode-specific permissions;
- choosing strategic behavior for uncontrolled actors;
- adding feature-rich UX that constrains future consumers.

## Startup behavior (current v0.6.2)

A true zero-pre-simulation start gate is **not part of the current core contract**: the game's Start Match transition needs real Candidate-A simulation progress before it can construct an interactive battlefield. Freezing at tick 1 or after only one complete tick was physically rejected.

The validated startup path provides a bounded loader runway at the standard 60 Hz pace, freezes the live watched Candidate-A simulation at the first usable InGame boundary, and waits for visible presentation to synchronize to that frozen state. Only then does the player see the READY prompt. `Ctrl+Home` is the **supported, explicit start command**, not a diagnostic escape; it is ignored until synchronization is ready. See `README.md`, `docs/known-issues.md`, and `docs/pacing-validation-log.md` for the implementation boundary and limitations.

Do not claim that control begins at literal simulation tick 1 or replace this path with a fixed time-offset guess.

## Ownership, temporary yield, and global release

`End` temporarily returns the selected champion to vanilla AI and the user to spectator control. It **does not** release live pacing or restore native replay seeking/highlight actions; those remain suppressed for the entire live-owned match, regardless of whether an athlete is currently selected.

Confirmed `Ctrl+End` is the base **one-way, global** release primitive. It relinquishes pacing and manual control, restores native replay actions, and allows the ordinary simulation to race ahead or finish. Live Direct Control cannot safely resume until a new match. Document that irreversible consequence wherever this shortcut appears.

Normal synchronized pause/resume remains supported. See `docs/replay-native-action-analysis.md` before touching replay-action ownership; shortcut suppression must survive user-remapped bindings.
