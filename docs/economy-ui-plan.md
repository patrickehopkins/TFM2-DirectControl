# Economy / shopping UI plan

> **October 2026 direction update:** Control v0.9 already implements a polished current-gold/native-next-purchase HUD, plus K/D, cooldown and selected-champion presentation. Harbinger should not reproduce that player-facing surface merely for parity. The remaining useful Harbinger problem is **native economy introspection and manual-shopping control as reusable infrastructure**.

This document records the original Direct Control economy-readability idea and the intended shopping-mode split.

## Superseded differentiation target: current gold HUD

The current-gold HUD was considered for the first release but deferred. It is **not shipped** and should not be treated as a pre-release requirement. If economy UI is revisited, display the currently controlled player's **current gold** somewhere clearly visible during live control.

Requirements:

- current gold must remain readable without opening Match Info;
- the value must track the live controlled player/team state rather than a cached estimate;
- it should work in both major match UI layouts;
- exact placement/styling can be chosen during the UI-polish pass, but it should be compact and continuously readable rather than buried in the existing diagnostic overlay.

This is no longer an active differentiator. Control demonstrates the concept successfully. Harbinger may still expose current gold as part of a generic economy API or minimal reference UI, but a dedicated polish pass should not be prioritized just to match Control.

## Later shop-control refinement

When Shop Control is implemented, preserve the game's normal automatic shopping as an explicit mod option rather than forcing every Direct Control user into manual shopping.

At minimum, the shopping-mode options should include:

- **Vanilla / Auto-Shop** — retain TFM2's normal automatic purchase/upgrade behavior.
- **Manual Shop** — Direct Control exposes the later manual-shopping interface/commands.

Do not globally disable vanilla automatic shopping merely because manual shopping exists.

## Auto-Shop economy helper — concept validated externally

When Auto-Shop is selected, the gold HUD should gain a compact next-purchase helper beside the current-gold value.

It should show:

- current gold;
- the **next item or item upgrade** the native auto-shop/build logic intends to buy;
- the gold requirement for that purchase/upgrade;
- preferably the remaining shortfall from current gold when the purchase is not yet affordable.

Illustrative presentation only:

```text
Gold: 320   Next: Long Sword (450)   Need: 130
```

Control v0.9 demonstrates essentially this exact concept successfully in a polished HUD. Treat that as external validation of the UX, not a mandate to duplicate its presentation.

For Harbinger, the important remaining requirement is that the **underlying native next-purchase intent, price, and shortfall be discoverable through reusable infrastructure** so manual shopping or other economy mods can build on it.

Use live/native shopping/build-path data and live prices wherever possible. Do not hard-code champion build paths, item progression, or prices into Harbinger merely to populate this helper.

## Scope placement

- **Current gold / next-purchase HUD parity:** deprioritized; Control already executes this well.
- **Economy introspection API:** still valuable.
- **Manual Shop:** still open and potentially valuable as a reusable control surface.
- Vanilla automatic shopping remains supported.
