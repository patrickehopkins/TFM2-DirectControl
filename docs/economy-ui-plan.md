# Economy / shopping UI plan

This document records the Direct Control economy-readability work and the intended shopping-mode split.

## Pre-release: current gold HUD

Before the Pings/team-command release boundary, Direct Control should display the currently controlled player's **current gold** somewhere clearly visible during live control.

Requirements:

- current gold must remain readable without opening Match Info;
- the value must track the live controlled player/team state rather than a cached estimate;
- it should work in both major match UI layouts;
- exact placement/styling can be chosen during the UI-polish pass, but it should be compact and continuously readable rather than buried in the existing diagnostic overlay.

This is a pre-release UI-polish item and should be completed immediately before the Pings/team-command boundary unless reprioritized explicitly.

## Later shop-control refinement

When Shop Control is implemented, preserve the game's normal automatic shopping as an explicit mod option rather than forcing every Direct Control user into manual shopping.

At minimum, the shopping-mode options should include:

- **Vanilla / Auto-Shop** — retain TFM2's normal automatic purchase/upgrade behavior.
- **Manual Shop** — Direct Control exposes the later manual-shopping interface/commands.

Do not globally disable vanilla automatic shopping merely because manual shopping exists.

## Auto-Shop economy helper

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

The exact visual format is deferred. The important requirement is that the player can immediately see both what the automatic shopper is waiting to buy next and how close they are to affording it.

Use live/native shopping/build-path data and live prices wherever possible. Do not hard-code champion build paths, item progression, or prices into Harbinger merely to populate this helper.

## Scope placement

- **Current gold HUD:** pre-release, immediately before Pings/team commands under the standing release-boundary rule.
- **Auto-Shop option + next-purchase helper:** refinement of the later Shop Control item; this does not by itself pull full manual shopping ahead of Pings.
