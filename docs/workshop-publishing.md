# Harbinger: in-game description vs Workshop copy

**Two different audiences; do not put the long comedic description in `mod.mod_info`.**

- `mod.mod_info.name`: **Harbinger Direct Control**, also shown in the game Mods menu.
- `mod.mod_info.description`: intentionally short and practical for the game Mods menu. The official TFM2 uploader also reads this as its initial/default Workshop description.
- `docs/workshop-description.txt`: complete marketing copy with jokes, shortcut instructions, caveats, and compatibility notes. This is **not** loaded automatically by the uploader.

## Existing-item release: v0.1.4

The copy in `docs/workshop-description.txt` is the **current v0.1.4
Workshop BBCode**, not the short in-game metadata. Preserve its formatting and
voice when applying updates. The Steam uploader can replace the Workshop page
description with `mod.mod_info.description`; check the live listing afterward
and paste the full BBCode back into **Edit title & description** if necessary.

For each update:

1. In GitHub Desktop, switch to `main` and pull the merged release.
   Follow **README.md → Release / Workshop** to bootstrap the SDK, run
   Cargo checks, and build `target\release\tfm2_direct_control.dll`.
2. Copy that fresh DLL and the repository's `mod.mod_info` into the **existing**
   `dist/workshop/tfm2_direct_control` package, as shown in the README.
   Verify matching DLL hashes and v0.1.4 metadata. Neither a stray root DLL
   nor a previously staged DLL is an authoritative build.
3. Preserve the original `mod.workshop_id` and existing preview/thumbnail
   assets. In `TFM2ModUploader.exe`, select the original publishing folder
   and run **Build Only (No Upload)**. Inspect the staged package. Build Only
   does **not** replace your separate Cargo build-and-copy process.
4. Smoke-test the intended package, then use **Update Workshop Item** on the
   **original** listing; do not start a fresh publication. Verify the live
   Workshop page and restore `docs/workshop-description.txt` if necessary.
   Use `docs/workshop-change-note-v0.1.4.txt` for the short change note.
5. When verifying the Workshop-installed copy, move the development
   installation outside the game's `mods` directory and restart TFM2.
   Never run two Harbinger installations simultaneously.

**Important:** Keep `dist/workshop/tfm2_direct_control/mod.workshop_id`
locally; it identifies the existing listing. Do not commit it to Git or
generate a new item by uploading without it.

For an initial publication only, the uploader initializes the Steam listing
from the short `mod.mod_info` description and adds its platform line. The
long Workshop copy must then be set manually on the Steam page. The existing
Harbinger listing should use the update procedure above.
