#!/usr/bin/env python3
"""Read-only TFM2 native replay/seek action discovery (current installed executable).

No game process is opened or modified. The output is a small text report with
string offsets and local context, not an executable or raw game assets.

Usage (repository root):
  py tools/probe_replay_actions.py
  py tools/probe_replay_actions.py --exe "D:\\SteamLibrary\\steamapps\\common\\Teamfight Manager2\\TeamfightManager2.exe"
"""

from __future__ import annotations

import argparse
import hashlib
import os
import re
import tempfile
from pathlib import Path

from scan_camera_action_xrefs import PeImage, find_substring_rvas, ascii_context

TERMS = (
    "in_game_rewind",
    "in_game_forward",
    "in_game_seek",
    "in_game_skip",
    "in_game_highlight",
    "rewind",
    "highlight",
    "seek",
    "playback",
    "camera_buttons",
    "zoom_in",
    "zoom_out",
)
MAX_TERM_HITS = 18
MAX_ACTION_STRINGS = 180
DEFAULT_GAME_DIR = (
    Path(os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)"))
    / "Steam" / "steamapps" / "common" / "Teamfight Manager2"
)


def printable(chunk: bytes) -> str:
    return "".join(chr(b) if 32 <= b < 127 else "." for b in chunk)


def scan(exe: Path) -> str:
    pe = PeImage(exe)
    data = pe.data
    sha = hashlib.sha256(data).hexdigest()
    lines = [
        "TFM2 DIRECT CONTROL — REPLAY ACTION DISCOVERY",
        "Read-only static inspection; no game files or process memory were changed.",
        f"Executable: {exe.name}",
        f"Size: {len(data)} bytes",
        f"SHA256: {sha}",
        f"PE timestamp: 0x{pe.timestamp:08X}",
        f"PE image size: 0x{pe.image_size:X}",
        "",
        "Unique candidate in_game_* string fragments:",
    ]

    found = {}
    for m in re.finditer(rb"in_game_[a-z0-9_]{3,80}", data):
        fragment = m.group().decode("ascii")
        if fragment in found:
            continue
        try:
            rva = pe.offset_to_rva(m.start())
        except ValueError:
            continue
        found[fragment] = rva

    for fragment, rva in list(found.items())[:MAX_ACTION_STRINGS]:
        lines.append(f"  RVA 0x{rva:08X}: {fragment}")
    if len(found) > MAX_ACTION_STRINGS:
        lines.append(f"  ... {len(found) - MAX_ACTION_STRINGS} more fragments omitted")

    lines.append("")
    for term in TERMS:
        occurrences = find_substring_rvas(pe, term)
        lines.append(f"=== {term}: {len(occurrences)} mapped occurrences ===")
        for rva in occurrences[:MAX_TERM_HITS]:
            lines.append(f"RVA 0x{rva:08X}")
            lines.append(ascii_context(pe, rva))
        if len(occurrences) > MAX_TERM_HITS:
            lines.append(f"  ... {len(occurrences) - MAX_TERM_HITS} more omitted")
        lines.append("")

    return "\n".join(lines) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", type=Path, help="Path to the installed TeamfightManager2.exe")
    parser.add_argument(
        "--output",
        type=Path,
        default=Path(tempfile.gettempdir()) / "tfm2_replay_actions_probe.txt",
    )
    args = parser.parse_args()

    exe = args.exe or (DEFAULT_GAME_DIR / "TeamfightManager2.exe")
    if not exe.is_file():
        raise SystemExit(f"Game executable not found: {exe} (use --exe for another Steam library)")
    report = scan(exe)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(report, encoding="utf-8")
    print(f"Replay-action report: {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
