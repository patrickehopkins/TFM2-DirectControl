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

**Important:** Keep `dist/workshop/tfm2_direct_control/mod.workshop_id` after first upload. The uploader uses it to update the same Steam item. Do not accidentally upload the new package as a second item.

The current promotional draft is reconstructed from earlier copy and known accepted jokes. If an exact user-edited joke is missing, amend `docs/workshop-description.txt` before final publication.
