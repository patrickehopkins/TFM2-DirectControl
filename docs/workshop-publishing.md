# Harbinger: in-game description vs Workshop copy

**Two different audiences; do not put the long comedic description in `mod.mod_info`.**

- `mod.mod_info.name`: **Harbinger Direct Control**, also shown in the game Mods menu.
- `mod.mod_info.description`: intentionally short and practical for the game Mods menu. The official TFM2 uploader also reads this as its initial/default Workshop description.
- `docs/workshop-description.txt`: complete marketing copy with jokes, shortcut instructions, caveats, and compatibility notes. This is **not** loaded automatically by the uploader.

To get different text in the two surfaces:

1. Stage a clean DLL + `mod.mod_info` + `thumbnail.png` + `preview.png` in `dist/workshop/tfm2_direct_control/`. Keep the internal mod ID and DLL filename unchanged.
2. Select that folder in `TFM2ModUploader.exe`, use **Build Only (No Upload)**, and inspect staged files/preview. This creates no Steam item.
3. Publish privately through **Publish to Steam Workshop**. The uploader initializes the Workshop listing from the *short* `mod.mod_info` metadata and adds its platform line.
4. Open your actual Steam Workshop item page, use the owner control **Edit title & description**, and paste the contents of `docs/workshop-description.txt` there. This changes the Steam listing, not the downloaded in-game metadata.
5. Verify the public-facing page before changing visibility to Public. After future uploader updates, check that the custom long Steam page description wasn't reset to the short `mod.mod_info` default; reapply it if necessary.

## Updating the existing Workshop item (v0.1.4)

1. Pull the approved release commit on `main`, then run the documented release build and smoke tests. Verify the DLL, `Cargo.toml`, `Cargo.lock`, and staged `mod.mod_info` all belong to v0.1.4.
2. In the uploader, select the **existing publishing folder that retains its original `mod.workshop_id`**. Use **Build Only (No Upload)** first and inspect the staged package, then **Update Workshop Item**, not a fresh publish.
3. The uploader may restore the short in-game `mod.mod_info` description to the Workshop listing. After upload, inspect the live listing and manually preserve/reapply the maintainer-edited long description if necessary; do not silently replace or revise the Workshop copy.

**Important:** Keep `dist/workshop/tfm2_direct_control/mod.workshop_id` after first upload. The uploader uses it to update the same Steam item. Do not accidentally upload the new package as a second item.

`docs/workshop-description.txt` contains the maintainer's final approved Workshop BBCode as committed on September 24, 2026. Do not silently rewrite its voice, formatting, jokes, or wording during technical documentation updates. Any future promotional-copy change needs an explicit editorial request.
