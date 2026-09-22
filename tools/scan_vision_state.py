#!/usr/bin/env python3
"""Read-only native vision-state/action probe for Teamfight Manager 2.

This is intentionally narrower than scan_camera_action_xrefs.py. It answers two
questions needed by Direct Control's automatic team fog feature:

1. Which object byte offsets are read near the native All / Blue / Red view-button
   UI code in the current executable?
2. Where do the binding-independent in_game_camera_all/team0/team1 action names
   feed native code?

The script only reads the executable and writes a text report.
"""

from __future__ import annotations

import argparse
import hashlib
import struct
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path

VIEW_ANCHORS = (
    "speed_buttons.view_all",
    "speed_buttons.view_blue",
    "speed_buttons.view_red",
)
ACTION_ANCHORS = (
    "in_game_camera_all",
    "in_game_camera_team0",
    "in_game_camera_team1",
)
CONTEXT_BEFORE = 0x80
CONTEXT_AFTER = 0xA0
CALL_RADIUS = 0x100


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
        pe = struct.unpack_from("<I", self.data, 0x3C)[0]
        if self.data[pe : pe + 4] != b"PE\0\0":
            raise ValueError("not a PE image")

        self.timestamp = struct.unpack_from("<I", self.data, pe + 8)[0]
        section_count = struct.unpack_from("<H", self.data, pe + 6)[0]
        optional_size = struct.unpack_from("<H", self.data, pe + 20)[0]
        optional = pe + 24
        self.image_base = struct.unpack_from("<Q", self.data, optional + 24)[0]
        self.image_size = struct.unpack_from("<I", self.data, optional + 56)[0]

        section_table = optional + optional_size
        self.sections: list[Section] = []
        for i in range(section_count):
            off = section_table + i * 40
            name = self.data[off : off + 8].rstrip(b"\0").decode("ascii", "replace")
            virtual_size, rva, raw_size, raw_offset = struct.unpack_from(
                "<IIII", self.data, off + 8
            )
            characteristics = struct.unpack_from("<I", self.data, off + 36)[0]
            self.sections.append(
                Section(name, rva, virtual_size, raw_offset, raw_size, characteristics)
            )

    def offset_to_rva(self, offset: int) -> int:
        for section in self.sections:
            if section.raw_offset <= offset < section.raw_offset + section.raw_size:
                return section.rva + offset - section.raw_offset
        raise ValueError("offset outside mapped section")

    def rva_to_offset(self, rva: int) -> int:
        for section in self.sections:
            if section.rva <= rva < section.rva + section.span:
                delta = rva - section.rva
                if delta >= section.raw_size:
                    raise ValueError("RVA in zero-filled section tail")
                return section.raw_offset + delta
        raise ValueError("RVA outside mapped section")

    def bytes_at(self, rva: int, size: int) -> bytes:
        off = self.rva_to_offset(rva)
        return self.data[off : off + size]


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def find_ascii_rvas(image: PeImage, text: str) -> list[int]:
    needle = text.encode("ascii")
    out: list[int] = []
    start = 0
    while True:
        off = image.data.find(needle, start)
        if off < 0:
            break
        try:
            out.append(image.offset_to_rva(off))
        except ValueError:
            pass
        start = off + 1
    return sorted(set(out))


def executable_rip_refs(image: PeImage) -> list[tuple[int, int, str]]:
    refs: list[tuple[int, int, str]] = []
    for section in image.sections:
        if not section.executable or section.raw_size < 7:
            continue
        data = image.data[section.raw_offset : section.raw_offset + section.raw_size]
        for i in range(len(data) - 7):
            rex_len = 1 if 0x40 <= data[i] <= 0x4F else 0
            op = i + rex_len
            if op + 6 > len(data) or data[op] not in (0x8D, 0x8B):
                continue
            modrm = data[op + 1]
            if (modrm & 0xC7) != 0x05:
                continue
            disp = struct.unpack_from("<i", data, op + 2)[0]
            insn_len = rex_len + 6
            insn_rva = section.rva + i
            refs.append(
                (insn_rva, insn_rva + insn_len + disp, "lea" if data[op] == 0x8D else "mov")
            )
    return refs


def nearby_calls(image: PeImage, center: int) -> list[tuple[int, int]]:
    out: list[tuple[int, int]] = []
    for section in image.sections:
        if not section.executable:
            continue
        start = max(section.rva, center - CALL_RADIUS)
        end = min(section.rva + section.raw_size, center + CALL_RADIUS)
        if end - start < 5:
            continue
        block = image.bytes_at(start, end - start)
        for i in range(len(block) - 4):
            if block[i] != 0xE8:
                continue
            disp = struct.unpack_from("<i", block, i + 1)[0]
            call = start + i
            out.append((call, call + 5 + disp))
    return sorted(set(out))


def byte_load_displacements(block: bytes, start_rva: int) -> list[tuple[int, int, str]]:
    """Heuristically decode MOV/MOVZX byte loads with disp8/disp32 addressing.

    We intentionally report candidates rather than pretending to recover full register
    semantics. Shared small displacements around all three view-button paths are the
    interesting result.
    """

    out: list[tuple[int, int, str]] = []
    i = 0
    while i < len(block) - 3:
        start = i
        if 0x40 <= block[i] <= 0x4F:
            i += 1

        kind = None
        if i < len(block) and block[i] == 0x8A:
            kind = "mov r8,[base+disp]"
            i += 1
        elif i + 1 < len(block) and block[i] == 0x0F and block[i + 1] == 0xB6:
            kind = "movzx r32,[base+disp]"
            i += 2
        else:
            i = start + 1
            continue

        if i >= len(block):
            break
        modrm = block[i]
        i += 1
        mod = modrm >> 6
        rm = modrm & 7

        # Skip SIB byte when present; we only need the displacement.
        if rm == 4 and mod != 3:
            if i >= len(block):
                break
            i += 1

        if mod == 1:
            if i >= len(block):
                break
            disp = struct.unpack_from("<b", block, i)[0]
            out.append((start_rva + start, disp, kind))
        elif mod == 2:
            if i + 4 > len(block):
                break
            disp = struct.unpack_from("<i", block, i)[0]
            out.append((start_rva + start, disp, kind))

        i = start + 1
    return out


def hexdump(data: bytes, start_rva: int) -> str:
    lines: list[str] = []
    for row in range(0, len(data), 16):
        chunk = data[row : row + 16]
        hx = " ".join(f"{b:02X}" for b in chunk)
        asc = "".join(chr(b) if 32 <= b <= 126 else "." for b in chunk)
        lines.append(f"    {start_rva + row:08X}  {hx:<47}  {asc}")
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--exe", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()

    image = PeImage(args.exe.resolve())
    refs = executable_rip_refs(image)
    by_target: dict[int, list[tuple[int, int, str]]] = defaultdict(list)
    for ref in refs:
        by_target[ref[1]].append(ref)

    lines = [
        "TFM2 DIRECT CONTROL - VISION STATE / ACTION PROBE",
        f"Executable: {image.path}",
        f"SHA-256: {sha256(image.data)}",
        f"PE timestamp: 0x{image.timestamp:08X}",
        f"Image size: 0x{image.image_size:08X}",
        "",
        "Read-only static analysis. No game files or process memory were modified.",
        "",
        "===== VIEW BUTTON STATE READS =====",
    ]

    displacement_hits: Counter[int] = Counter()
    displacement_labels: defaultdict[int, set[str]] = defaultdict(set)

    for label in VIEW_ANCHORS:
        lines.append(f"\n--- {label} ---")
        string_rvas = find_ascii_rvas(image, label)
        if not string_rvas:
            lines.append("string not found")
            continue

        any_ref = False
        for string_rva in string_rvas:
            code_refs = by_target.get(string_rva, [])
            lines.append(f"string RVA 0x{string_rva:08X}; direct refs={len(code_refs)}")
            for xref, _, kind in code_refs:
                any_ref = True
                start = max(0, xref - CONTEXT_BEFORE)
                size = CONTEXT_BEFORE + CONTEXT_AFTER
                try:
                    block = image.bytes_at(start, size)
                except ValueError:
                    continue
                loads = byte_load_displacements(block, start)
                lines.append(f"  XREF 0x{xref:08X} ({kind})")
                if loads:
                    lines.append("  nearby byte-field loads:")
                    for insn, disp, load_kind in loads:
                        lines.append(
                            f"    0x{insn:08X}: {load_kind} disp={disp:+#x}"
                        )
                        if -0x1000 <= disp <= 0x1000:
                            displacement_hits[disp] += 1
                            displacement_labels[disp].add(label)
                else:
                    lines.append("  nearby byte-field loads: <none>")
                lines.append("  byte context:")
                lines.append(hexdump(block, start))

        if not any_ref:
            lines.append("no direct executable refs to this substring")

    lines.append("\n===== SHARED VIEW-STATE DISPLACEMENT CANDIDATES =====")
    shared = [
        (disp, count, displacement_labels[disp])
        for disp, count in displacement_hits.items()
        if len(displacement_labels[disp]) >= 2
    ]
    shared.sort(key=lambda item: (-len(item[2]), -item[1], abs(item[0]), item[0]))
    if not shared:
        lines.append("<none>")
    else:
        for disp, count, labels in shared[:50]:
            lines.append(
                f"disp={disp:+#x} hits={count} anchors={','.join(sorted(labels))}"
            )

    lines.append("\n===== CAMERA ACTION ROUTES =====")
    for label in ACTION_ANCHORS:
        lines.append(f"\n--- {label} ---")
        string_rvas = find_ascii_rvas(image, label)
        if not string_rvas:
            lines.append("string not found")
            continue
        for string_rva in string_rvas:
            code_refs = by_target.get(string_rva, [])
            lines.append(f"string RVA 0x{string_rva:08X}; direct refs={len(code_refs)}")
            for xref, _, kind in code_refs:
                lines.append(f"  XREF 0x{xref:08X} ({kind})")
                calls = nearby_calls(image, xref)
                for call, target in calls:
                    marker = " <== near xref" if abs(call - xref) <= 0x30 else ""
                    lines.append(
                        f"    CALL 0x{call:08X} -> 0x{target:08X}{marker}"
                    )

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text("\n".join(lines), encoding="utf-8")
    print(args.output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
