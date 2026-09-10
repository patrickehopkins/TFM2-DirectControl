#!/usr/bin/env python3
"""Read-only string/symbol probe for Teamfight Manager 2 camera discovery.

Scans the game's top-level PE files for printable ASCII/UTF-16LE strings that
look related to the spectator camera, projection, zoom, or render state. This
never modifies the game installation or the running process.
"""

from __future__ import annotations

import argparse
import hashlib
import re
from pathlib import Path

ASCII_RE = re.compile(rb"[\x20-\x7e]{4,}")
UTF16_RE = re.compile(rb"(?:[\x20-\x7e]\x00){4,}")

KEYWORDS = (
    "camera",
    "zoom",
    "viewport",
    "view_port",
    "renderstate",
    "render_state",
    "screen_to_world",
    "world_to_screen",
    "screentoworld",
    "worldtoscreen",
    "orthographic",
    "projection",
    "spectator",
    "spectate",
    "camera_target",
    "camera_position",
    "camera_pos",
    "camera_scale",
    "camera_zoom",
    "game_camera",
    "match_camera",
    "view_transform",
    "view_projection",
)

MAX_MATCHES_PER_FILE = 2000
MAX_STRING_CHARS = 900
CONTEXT_STRINGS = 2


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def clean(text: str) -> str:
    text = text.replace("\r", " ").replace("\n", " ").replace("\t", " ")
    if len(text) > MAX_STRING_CHARS:
        return text[:MAX_STRING_CHARS] + "..."
    return text


def extract_strings(data: bytes) -> list[tuple[int, str, str]]:
    found: list[tuple[int, str, str]] = []

    for match in ASCII_RE.finditer(data):
        found.append((match.start(), "ascii", match.group().decode("ascii", errors="replace")))

    for match in UTF16_RE.finditer(data):
        found.append(
            (
                match.start(),
                "utf16le",
                match.group().decode("utf-16le", errors="replace"),
            )
        )

    found.sort(key=lambda item: (item[0], item[1]))
    return found


def is_interesting(text: str) -> bool:
    lower = text.lower()
    return any(keyword in lower for keyword in KEYWORDS)


def candidate_files(game_dir: Path) -> list[Path]:
    # Camera code should live in executable modules. Keep this deliberately
    # top-level so we do not waste time scanning our own mods or SDK copies.
    allowed = {".exe", ".dll", ".pdb"}
    files = [
        path
        for path in game_dir.iterdir()
        if path.is_file() and path.suffix.lower() in allowed
    ]
    return sorted(files, key=lambda path: path.name.lower())


def scan_file(path: Path) -> list[str]:
    lines: list[str] = []
    data = path.read_bytes()
    strings = extract_strings(data)

    lines.append(f"===== {path.name} =====")
    lines.append(f"Path: {path}")
    lines.append(f"Size: {len(data)} bytes")
    lines.append(f"SHA256: {sha256(path)}")
    lines.append(f"Printable strings: {len(strings)}")

    pdb_strings = [item for item in strings if ".pdb" in item[2].lower()]
    if pdb_strings:
        lines.append("PDB/debug-path candidates:")
        for offset, encoding, text in pdb_strings[:50]:
            lines.append(f"  0x{offset:08x} [{encoding}] {clean(text)}")
    else:
        lines.append("PDB/debug-path candidates: <none>")

    match_indices = [i for i, (_, _, text) in enumerate(strings) if is_interesting(text)]
    lines.append(f"Camera/render keyword matches: {len(match_indices)}")
    lines.append("")

    for ordinal, index in enumerate(match_indices[:MAX_MATCHES_PER_FILE], start=1):
        offset, encoding, text = strings[index]
        lines.append(
            f"MATCH {ordinal:04d} @ 0x{offset:08x} [{encoding}] {clean(text)}"
        )
        start = max(0, index - CONTEXT_STRINGS)
        end = min(len(strings), index + CONTEXT_STRINGS + 1)
        for context_index in range(start, end):
            if context_index == index:
                continue
            c_offset, c_encoding, c_text = strings[context_index]
            delta = context_index - index
            lines.append(
                f"    ctx {delta:+d} 0x{c_offset:08x} [{c_encoding}] {clean(c_text)}"
            )
        lines.append("")

    if len(match_indices) > MAX_MATCHES_PER_FILE:
        lines.append(
            f"[TRUNCATED: {len(match_indices) - MAX_MATCHES_PER_FILE} additional matches]"
        )
        lines.append("")

    return lines


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--game-dir", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()

    game_dir = args.game_dir.resolve()
    output = args.output.resolve()

    if not game_dir.is_dir():
        raise SystemExit(f"Game directory does not exist: {game_dir}")

    files = candidate_files(game_dir)
    if not files:
        raise SystemExit(f"No top-level .exe/.dll/.pdb files found in: {game_dir}")

    report: list[str] = [
        "TFM2 DIRECT CONTROL - CAMERA STRING/SYMBOL DISCOVERY REPORT",
        f"Game directory: {game_dir}",
        f"Files scanned: {len(files)}",
        "Keywords: " + ", ".join(KEYWORDS),
        "",
        "This is a read-only static scan. No game files were modified.",
        "",
    ]

    for path in files:
        try:
            report.extend(scan_file(path))
        except Exception as exc:  # keep scanning the remaining modules
            report.append(f"===== {path.name} =====")
            report.append(f"ERROR: {exc}")
            report.append("")

    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text("\n".join(report), encoding="utf-8")
    print(output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
