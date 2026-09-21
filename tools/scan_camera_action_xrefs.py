#!/usr/bin/env python3
"""Read-only native action xref probe for Teamfight Manager 2.

Locates the spectator-camera action strings in TeamfightManager2.exe, then scans
executable sections for RIP-relative references to those exact strings. For each
xref it prints nearby bytes and rel32 call targets. The goal is to identify the
semantic action registry/dispatcher used by:

    in_game_camera_all
    in_game_camera_team0
    in_game_camera_team1
    in_game_auto_follow

No game files or process memory are modified.
"""

from __future__ import annotations

import argparse
import hashlib
import struct
from dataclasses import dataclass
from pathlib import Path


ACTION_NAMES = (
    "in_game_camera_all",
    "in_game_camera_team0",
    "in_game_camera_team1",
    "in_game_auto_follow",
)

CONTEXT_BEFORE = 0x50
CONTEXT_AFTER = 0x90
CALL_SCAN_BEFORE = 0x80
CALL_SCAN_AFTER = 0x100


@dataclass(frozen=True)
class Section:
    name: str
    rva: int
    virtual_size: int
    raw_offset: int
    raw_size: int
    characteristics: int

    @property
    def span(self) -> int:
        return max(self.virtual_size, self.raw_size)

    @property
    def executable(self) -> bool:
        return bool(self.characteristics & 0x20000000)


class PeImage:
    def __init__(self, path: Path) -> None:
        self.path = path
        self.data = path.read_bytes()

        pe_offset = struct.unpack_from("<I", self.data, 0x3C)[0]
        if self.data[pe_offset : pe_offset + 4] != b"PE\0\0":
            raise ValueError("not a PE image")

        self.timestamp = struct.unpack_from("<I", self.data, pe_offset + 8)[0]
        section_count = struct.unpack_from("<H", self.data, pe_offset + 6)[0]
        optional_size = struct.unpack_from("<H", self.data, pe_offset + 20)[0]
        optional_offset = pe_offset + 24
        self.image_base = struct.unpack_from("<Q", self.data, optional_offset + 24)[0]
        self.image_size = struct.unpack_from("<I", self.data, optional_offset + 56)[0]

        section_offset = optional_offset + optional_size
        sections: list[Section] = []
        for index in range(section_count):
            offset = section_offset + index * 40
            name = self.data[offset : offset + 8].rstrip(b"\0").decode("ascii", "replace")
            virtual_size, virtual_address, raw_size, raw_offset = struct.unpack_from(
                "<IIII", self.data, offset + 8
            )
            characteristics = struct.unpack_from("<I", self.data, offset + 36)[0]
            sections.append(
                Section(
                    name=name,
                    rva=virtual_address,
                    virtual_size=virtual_size,
                    raw_offset=raw_offset,
                    raw_size=raw_size,
                    characteristics=characteristics,
                )
            )
        self.sections = sections

    def offset_to_rva(self, offset: int) -> int:
        for section in self.sections:
            if section.raw_offset <= offset < section.raw_offset + section.raw_size:
                return section.rva + (offset - section.raw_offset)
        if 0 <= offset < min((s.raw_offset for s in self.sections), default=len(self.data)):
            return offset
        raise ValueError(f"file offset 0x{offset:X} is outside mapped sections")

    def rva_to_offset(self, rva: int) -> int:
        for section in self.sections:
            if section.rva <= rva < section.rva + section.span:
                delta = rva - section.rva
                if delta >= section.raw_size:
                    raise ValueError(f"RVA 0x{rva:X} is in zero-filled section tail")
                return section.raw_offset + delta
        raise ValueError(f"RVA 0x{rva:X} is outside mapped sections")

    def bytes_at_rva(self, rva: int, size: int) -> bytes:
        offset = self.rva_to_offset(rva)
        return self.data[offset : offset + size]


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def hexdump(data: bytes, start_rva: int) -> str:
    lines: list[str] = []
    for row in range(0, len(data), 16):
        chunk = data[row : row + 16]
        hex_part = " ".join(f"{b:02X}" for b in chunk)
        ascii_part = "".join(chr(b) if 32 <= b <= 126 else "." for b in chunk)
        lines.append(f"    {start_rva + row:08X}  {hex_part:<47}  {ascii_part}")
    return "\n".join(lines)


def find_ascii_string_rvas(image: PeImage, text: str) -> list[int]:
    needle = text.encode("ascii") + b"\0"
    out: list[int] = []
    start = 0
    while True:
        offset = image.data.find(needle, start)
        if offset < 0:
            break
        try:
            out.append(image.offset_to_rva(offset))
        except ValueError:
            pass
        start = offset + 1
    return out


def rip_refs_to(image: PeImage, target_rva: int) -> list[tuple[int, str]]:
    """Find common x64 RIP-relative LEA/MOV references to target_rva.

    Supports optional REX prefixes and ModRM mod=00,r/m=101 encodings for
    LEA (8D) and MOV (8B). This is intentionally conservative and does not try
    to be a complete x86 decoder.
    """

    refs: list[tuple[int, str]] = []

    for section in image.sections:
        if not section.executable or section.raw_size < 7:
            continue

        data = image.data[section.raw_offset : section.raw_offset + section.raw_size]
        for i in range(len(data) - 7):
            rex_len = 1 if 0x40 <= data[i] <= 0x4F else 0
            opcode_index = i + rex_len
            if opcode_index + 6 > len(data):
                continue
            opcode = data[opcode_index]
            if opcode not in (0x8D, 0x8B):
                continue
            modrm = data[opcode_index + 1]
            if (modrm & 0xC7) != 0x05:
                continue

            disp = struct.unpack_from("<i", data, opcode_index + 2)[0]
            insn_len = rex_len + 6
            insn_rva = section.rva + i
            resolved = insn_rva + insn_len + disp
            if resolved != target_rva:
                continue

            refs.append((insn_rva, "lea" if opcode == 0x8D else "mov"))

    return refs


def nearby_rel32_calls(image: PeImage, center_rva: int) -> list[tuple[int, int]]:
    out: list[tuple[int, int]] = []
    start_rva = max(0, center_rva - CALL_SCAN_BEFORE)
    end_rva = center_rva + CALL_SCAN_AFTER

    for section in image.sections:
        if not section.executable:
            continue
        scan_start = max(start_rva, section.rva)
        scan_end = min(end_rva, section.rva + section.raw_size)
        if scan_end - scan_start < 5:
            continue

        try:
            block = image.bytes_at_rva(scan_start, scan_end - scan_start)
        except ValueError:
            continue

        for i in range(len(block) - 4):
            if block[i] != 0xE8:
                continue
            disp = struct.unpack_from("<i", block, i + 1)[0]
            call_rva = scan_start + i
            target = call_rva + 5 + disp
            out.append((call_rva, target))

    # Preserve order while removing duplicates from overlapping scan regions.
    seen: set[tuple[int, int]] = set()
    unique: list[tuple[int, int]] = []
    for item in sorted(out):
        if item not in seen:
            seen.add(item)
            unique.append(item)
    return unique


def dump_xref_context(image: PeImage, xref_rva: int) -> str:
    for section in image.sections:
        if section.rva <= xref_rva < section.rva + section.raw_size:
            start = max(section.rva, xref_rva - CONTEXT_BEFORE)
            end = min(section.rva + section.raw_size, xref_rva + CONTEXT_AFTER)
            try:
                return hexdump(image.bytes_at_rva(start, end - start), start)
            except ValueError:
                return "<unavailable>"
    return "<xref outside executable section>"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("exe", type=Path, help="path to TeamfightManager2.exe")
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("camera-action-xrefs.txt"),
        help="report path (default: camera-action-xrefs.txt)",
    )
    args = parser.parse_args()

    image = PeImage(args.exe.resolve())

    lines: list[str] = [
        "TFM2 DIRECT CONTROL - SPECTATOR CAMERA ACTION XREF REPORT",
        f"Executable: {image.path}",
        f"SHA-256: {sha256(image.data)}",
        f"PE timestamp: 0x{image.timestamp:08X}",
        f"Image size: 0x{image.image_size:08X}",
        f"Image base: 0x{image.image_base:X}",
        "",
        "Read-only static analysis. No game files or process memory were modified.",
        "",
    ]

    all_xrefs: dict[str, list[int]] = {}

    for name in ACTION_NAMES:
        string_rvas = find_ascii_string_rvas(image, name)
        lines.append(f"===== {name} =====")
        if not string_rvas:
            lines.append("String: NOT FOUND")
            lines.append("")
            all_xrefs[name] = []
            continue

        name_xrefs: list[int] = []
        for string_rva in string_rvas:
            lines.append(f"String RVA: 0x{string_rva:08X}")
            refs = rip_refs_to(image, string_rva)
            if not refs:
                lines.append("RIP-relative executable refs: <none>")
                continue

            for xref_rva, kind in refs:
                name_xrefs.append(xref_rva)
                lines.append(f"XREF: 0x{xref_rva:08X} ({kind})")
                calls = nearby_rel32_calls(image, xref_rva)
                if calls:
                    lines.append("Nearby rel32 calls:")
                    for call_rva, target in calls:
                        marker = "  <== near xref" if abs(call_rva - xref_rva) <= 0x20 else ""
                        lines.append(
                            f"  CALL 0x{call_rva:08X} -> 0x{target:08X}{marker}"
                        )
                else:
                    lines.append("Nearby rel32 calls: <none>")
                lines.append("Byte context:")
                lines.append(dump_xref_context(image, xref_rva))
                lines.append("")

        all_xrefs[name] = sorted(set(name_xrefs))
        lines.append("")

    lines.append("===== XREF CLUSTERS =====")
    flattened = sorted(
        (xref, name)
        for name, xrefs in all_xrefs.items()
        for xref in xrefs
    )
    if not flattened:
        lines.append("<no executable string xrefs found>")
    else:
        for xref, name in flattened:
            neighbors = [
                (other_xref, other_name)
                for other_xref, other_name in flattened
                if other_name != name and abs(other_xref - xref) <= 0x400
            ]
            neighbor_text = ", ".join(
                f"{other_name}@0x{other_xref:08X}" for other_xref, other_name in neighbors
            )
            lines.append(
                f"{name}@0x{xref:08X}: {neighbor_text if neighbor_text else '<no other action xref within 0x400>'}"
            )

    output = args.output.resolve()
    output.write_text("\n".join(lines), encoding="utf-8")
    print(output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
