#!/usr/bin/env python3
"""Read-only 0.6.1 replay-action xref probe.

This goes beyond the simple string inventory: it locates native code referencing
the replay action names and their Rust string descriptors. The results help
identify an action-level gate that survives key remapping. This probe does not
install hooks, modify files, or inspect a running game process.
"""
from __future__ import annotations

import argparse
import hashlib
import os
import tempfile
from pathlib import Path

from scan_camera_action_xrefs import (
    PeImage, collect_rip_refs, find_substring_rvas, find_pointer_occurrences,
    descriptor_kind, refs_to, refs_near, emit_code_ref,
    emit_descriptor_table_near, emit_code_window,
)

ACTIONS = (
    "in_game_prev_time",
    "in_game_next_time",
    "in_game_prev_highlight",
    "in_game_next_highlight",
    "in_game_highlight_mode",
    "in_game_pause_time",
    "in_game_zoom_in",
    "in_game_zoom_out",
)
UI_PATHS = (
    "time_control.prev_time",
    "time_control.next_time",
    "time_control.prev_highlight",
    "time_control.next_highlight",
)
DEFAULT_EXE = (
    Path(os.environ.get("ProgramFiles(x86)", "C:\\Program Files (x86)"))
    / "Steam" / "steamapps" / "common" / "Teamfight Manager2"
    / "TeamfightManager2.exe"
)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", type=Path, default=DEFAULT_EXE)
    parser.add_argument(
        "--output", type=Path,
        default=Path(tempfile.gettempdir()) / "tfm2_replay_dispatch_probe.txt",
    )
    args = parser.parse_args()
    exe = args.exe.resolve()
    if not exe.is_file():
        raise SystemExit(f"Executable not found: {exe} (specify --exe for another library)")

    image = PeImage(exe)
    all_refs = collect_rip_refs(image)
    lines = [
        "TFM2 Replay Dispatch Xref Probe (read-only)",
        f"SHA256: {hashlib.sha256(image.data).hexdigest()}",
        f"PE timestamp: 0x{image.timestamp:08X}",
        f"PE image size: 0x{image.image_size:X}",
        f"RIP-relative references scanned: {len(all_refs)}",
        "",
        "Xrefs are candidate evidence, not independently verified native action handlers.",
        "",
    ]
    action_refs: dict[str, set[int]] = {}

    for name in (*ACTIONS, *UI_PATHS):
        lines.append(f"===== {name} =====")
        hits = find_substring_rvas(image, name)
        lines.append(f"Raw string occurrences: {len(hits)}")
        direct_count = 0
        descriptor_count = 0
        for string_rva in hits[:6]:
            lines.append(f"String RVA: 0x{string_rva:08X}")
            direct = refs_to(all_refs, string_rva)
            for ref_rva, data_rva, kind in direct[:4]:
                direct_count += 1
                action_refs.setdefault(name, set()).add(ref_rva)
                emit_code_ref(lines, image, ref_rva, data_rva, kind, "DIRECT STRING XREF")

            for desc_rva in find_pointer_occurrences(image, string_rva)[:6]:
                kind = descriptor_kind(image, desc_rva, len(name))
                # Require an exact Rust string length to eliminate random pointer hits.
                if kind != "Rust &str {ptr, usize_len}":
                    continue
                descriptor_count += 1
                lines.append(f"Rust &str descriptor RVA: 0x{desc_rva:08X}")
                exact = refs_to(all_refs, desc_rva)
                near = sorted(
                    refs_near(all_refs, desc_rva, radius=0x80),
                    key=lambda ref: (abs(ref[1] - desc_rva), ref[0]),
                )
                candidates = exact[:4] if exact else near[:4]
                if not candidates:
                    lines.append("  No local code references found.")
                for ref_rva, data_rva, ref_kind in candidates:
                    action_refs.setdefault(name, set()).add(ref_rva)
                    emit_code_ref(lines, image, ref_rva, data_rva, ref_kind, "DESCRIPTOR CODE XREF")

                if name == "in_game_prev_time":
                    emit_descriptor_table_near(lines, image, all_refs, desc_rva, radius=0x130)

        lines.append(f"Direct code references shown: {direct_count}")
        lines.append(f"Verified Rust descriptors shown: {descriptor_count}")
        lines.append("")

    lines.append("===== RELATED CODE XREF CLUSTERS =====")
    action_items = sorted(
        (addr, name) for name, addresses in action_refs.items() for addr in addresses
    )
    for addr, name in action_items[:100]:
        nearby = sorted(
            (other_addr, other_name)
            for other_addr, other_name in action_items
            if other_name != name and abs(other_addr - addr) < 0x350
        )
        if nearby:
            lines.append(
                f"0x{addr:08X} {name}: "
                + ", ".join(f"{n}@0x{a:08X}" for a, n in nearby[:8])
            )

    # A limited code window near overlapping refs can help distinguish shortcut
    # enum/name conversion from the actual replay-action dispatch loop.
    clusters = [
        addr for addr, name in action_items
        if name == "in_game_prev_time" and
        any(other != name and abs(other_addr - addr) < 0x120
            for other_addr, other in action_items)
    ]
    for addr in sorted(set(clusters))[:2]:
        emit_code_window(
            lines, image, f"RELATED ACTION XREF WINDOW near 0x{addr:08X}",
            max(0, addr - 0x90), addr + 0x160,
        )

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"Replay dispatch report: {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
