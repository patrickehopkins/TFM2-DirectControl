# Harbinger: one source, two build destinations

**Source of truth: GitHub repository.** Never edit or select `target/`, the
game's `mods/` directory, or `dist/` as source. They contain copies produced
from the checked-out repository. Only the one existing Workshop publishing
directory preserves its uploader identity and artwork.

Use **one command**, with one of two targets, from the repository root:

| Work | Command | Generated destination |
| --- | --- | --- |
| Test a branch locally | `.\scripts\build-mod.ps1 -Target Dev` | `<Game>\mods\tfm2_direct_control\` |
| Prepare an existing Workshop update | `.\scripts\build-mod.ps1 -Target Workshop` | `dist\workshop\tfm2_direct_control\` |

The script builds the DLL from the repository each time. **Neither destination
is a source of code.** `target\release\tfm2_direct_control.dll` is Cargo's
transient compilation result, not another authoritative build to manage.

## Everyday development

1. In GitHub Desktop, switch to the feature branch and pull.
2. In the repository's PowerShell, run:
   ```powershell
   .\scripts\build-mod.ps1 -Target Dev
   ```
3. Test the local mod in game. Disable/unsubscribe from the Workshop version
   during local testing; TFM2 will refuse to load duplicate native modules.
4. Commit/push changes to the branch. Merge the PR only after the requested
   approval and physical acceptance checks.

This entry point delegates to the existing `install-dev.ps1`; that older
script remains supported for existing contributor instructions.

## Every Workshop update

1. Merge the approved feature PR through GitHub.
2. In GitHub Desktop **switch to main, Fetch origin, Pull origin**. Ensure the
   working tree is clean. It is not enough to have built a feature branch.
3. Run **exactly this**, from the repository root:
   ```powershell
   .\scripts\build-mod.ps1 -Target Workshop
   ```
   The script verifies `main` matches `origin/main`, checks a clean working
   tree, verifies `mod.mod_info` matches Cargo's version, and checks the
   existing Workshop identity and artwork. It then bootstraps the installed
   game's SDK, runs `cargo fmt --check` and `cargo test`, forces a clean
   default-feature release rebuild (avoiding experimental native tracing),
   and copies the freshly built DLL and canonical metadata directly into the
   **original publishing folder**. It checks the compiled/staged DLL hashes.
   If any step fails, it stops before announcing that the package is ready.
4. Open the game's `TFM2ModUploader.exe` and **always choose**:
   ```text
   <GitHub repository>\dist\workshop\tfm2_direct_control
   ```
   Click **Refresh**. Verify your original Workshop item is shown, **not**
   `New item`, and check the intended version and preview image. `Native
   code: None` is expected for this *precompiled* DLL package; leave
   `Build native Rust code before uploading` unchecked.
5. Click **Build Only (No Upload)**, then review the uploader's actual staged
   file set. Its log must identify `tfm2_direct_control.dll`; there must
   be no source, SDK, tracing builds, unexpected files, or missing artwork.
   The script also writes `dist\workshop\RELEASE-MANIFEST.txt` **outside**
   the publishing folder, recording the exact Git commit, version, Workshop
   item and DLL SHA256 for your own reference.
6. Enter a short change note and choose **Update Workshop Item**.
   Do not use Publish New. Preserve the hand-edited Steam listing description.
7. If verifying the subscriber install, **first remove the local development
   mod folder** under `<Game>\mods\tfm2_direct_control`, let Steam download
   the updated Workshop item, and restart the game. That local deletion does
   not remove your source, publishing folder, or Steam Workshop item.

**Never copy a DLL manually between these folders again.** If you need a new
developer build, run `-Target Dev`. If you need a new published build, run
`-Target Workshop`.

## One-time publishing identity

The original `dist\workshop\tfm2_direct_control` directory should already
contain:

- `mod.workshop_id`: the existing Workshop listing ID,
- `preview.png`: its existing preview artwork,
- `mod.mod_info` and `tfm2_direct_control.dll`: *generated copies* that
  the `-Target Workshop` script replaces.

The ID and artwork are the only pieces retained from release to release.
Do not delete them or commit `mod.workshop_id` to Git. `dist/` is
Git-ignored. On first successful packaging, the script also keeps a one-time
**emergency backup** of the identity and preview at:

```text
%USERPROFILE%\Documents\Harbinger-Publishing-Backup\
```

If Documents is redirected by Windows, the actual path printed by the script
takes precedence. The backup is **not** an alternative publish directory.
If the original publishing directory is lost, restore those two files before
running the script; it will never silently create a new Workshop listing.

The script is project-specific and checks the original listing ID
(`3807134574`). If it encounters a different identity or unexpected extra
files in the package folder, it stops for manual inspection.

### When the game updates

Do **not** treat a successful build alone as compatibility verification.
Native detours are fingerprinted to tested executables. Validate the new
game executable and SDK separately before publishing an updated DLL.
