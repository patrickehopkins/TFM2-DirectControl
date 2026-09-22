#!/usr/bin/env python3
"""Read-only native speed-state probe for Teamfight Manager 2.

The Direct Control speed feature must follow TFM2's existing 0.5x/1x/1.5x/2x/3x
presentation controls rather than inventing a second input system. This probe traces
both the semantic action names and the live speed-button UI paths, then highlights
nearby small-enum and float-speed writes.

It never modifies the executable or game process.
"""

from __future__ import annotations

import argparse
import hashlib
import struct
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path

ACTION_ANCHORS = (
    "in_game_speed_half",
    "in_game_speed1",
    "in_game_speed15",
    "in_game_speed2",
    "in_game_speed3",
    "in_game_highlight_mode",
)

UI_ANCHORS = (
    "speed_buttons.speed05x",
    "speed_buttons.speed1x",
    "speed_buttons.speed15x",
    "speed_buttons.speed2x",
    "speed_buttons.speed3x",
    "speed_buttons.speed_highlight",
)

CONTEXT_BEFORE = 0x100
CONTEXT_AFTER = 0x180
NEAR_DESCRIPTOR_RADIUS = 0x180
WRITE_SCAN_RADIUS = 0x500

FLOAT_BITS = {
    0x3F000000: "0.5f",
    0x3F800000: "1.0f",
    0x3FC00000: "1.5f",
    0x40000000: "2.0f",
    0x40400000: "3.0f",
}


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
        for s in self.sections:
            if s.raw_offset <= offset < s.raw_offset + s.raw_size:
                return s.rva + offset - s.raw_offset
        raise ValueError("offset outside mapped section")

    def rva_to_offset(self, rva: int) -> int:
        for s in self.sections:
            if s.rva <= rva < s.rva + s.span:
                delta = rva - s.rva
                if delta >= s.raw_size:
                    raise ValueError("RVA in zero-filled section tail")
                return s.raw_offset + delta
        raise ValueError("RVA outside mapped section")

    def bytes_at(self, rva: int, size: int) -> bytes:
        off = self.rva_to_offset(rva)
        return self.data[off : off + size]


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def hexdump(data: bytes, start_rva: int) -> str:
    lines: list[str] = []
    for row in range(0, len(data), 16):
        chunk = data[row : row + 16]
        hx = " ".join(f"{b:02X}" for b in chunk)
        asc = "".join(chr(b) if 32 <= b <= 126 else "." for b in chunk)
        lines.append(f"    {start_rva + row:08X}  {hx:<47}  {asc}")
    return "\n".join(lines)


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


def find_pointer_occurrences(image: PeImage, target_rva: int) -> list[int]:
    needle = struct.pack("<Q", image.image_base + target_rva)
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


def pdata_runtime_functions(image: PeImage) -> list[tuple[int, int]]:
    pdata = next((s for s in image.sections if s.name == ".pdata"), None)
    if pdata is None:
        return []
    raw = image.data[pdata.raw_offset : pdata.raw_offset + pdata.raw_size]
    out: list[tuple[int, int]] = []
    for i in range(0, len(raw) - 11, 12):
        begin, end, unwind = struct.unpack_from("<III", raw, i)
        if begin and begin < end:
            out.append((begin, end))
    out.sort()
    return out


def owning_function(funcs: list[tuple[int, int]], rva: int) -> tuple[int, int] | None:
    lo, hi = 0, len(funcs)
    while lo < hi:
        mid = (lo + hi) // 2
        begin, end = funcs[mid]
        if rva < begin:
            hi = mid
        elif rva >= end:
            lo = mid + 1
        else:
            return begin, end
    return None


def parse_mem_operand(data: bytes, op_index: int) -> tuple[int | None, int] | None:
    """Return (displacement, next_index) for a ModRM memory operand.

    This intentionally only decodes enough x64 addressing to identify disp8/disp32
    field writes. RIP-relative mode is returned as displacement None because it is
    not an object-relative field candidate.
    """

    if op_index >= len(data):
        return None
    modrm = data[op_index]
    mod = modrm >> 6
    rm = modrm & 7
    if mod == 3:
        return None

    i = op_index + 1
    base_rm = rm
    if rm == 4:
        if i >= len(data):
            return None
        sib = data[i]
        base_rm = sib & 7
        i += 1

    if mod == 0:
        if base_rm == 5:
            if i + 4 > len(data):
                return None
            return None, i + 4
        return 0, i
    if mod == 1:
        if i >= len(data):
            return None
        return struct.unpack_from("<b", data, i)[0], i + 1
    if mod == 2:
        if i + 4 > len(data):
            return None
        return struct.unpack_from("<i", data, i)[0], i + 4
    return None


def candidate_writes(block: bytes, start_rva: int) -> list[tuple[int, int | None, str, str]]:
    """Find likely enum/float writes to object fields in a code window."""

    hits: list[tuple[int, int | None, str, str]] = []
    for start in range(max(0, len(block) - 2)):
        i = start
        if i < len(block) and 0x40 <= block[i] <= 0x4F:
            i += 1
        if i >= len(block):
            continue

        # C6 /0 r/m8, imm8
        if block[i] == 0xC6 and i + 2 < len(block):
            modrm = block[i + 1]
            if ((modrm >> 3) & 7) != 0:
                continue
            parsed = parse_mem_operand(block, i + 1)
            if parsed is None:
                continue
            disp, next_i = parsed
            if next_i >= len(block):
                continue
            imm = block[next_i]
            if imm <= 8:
                hits.append((start_rva + start, disp, f"enum {imm}", "mov byte [mem], imm8"))

        # C7 /0 r/m32, imm32
        if block[i] == 0xC7 and i + 5 < len(block):
            modrm = block[i + 1]
            if ((modrm >> 3) & 7) != 0:
                continue
            parsed = parse_mem_operand(block, i + 1)
            if parsed is None:
                continue
            disp, next_i = parsed
            if next_i + 4 > len(block):
                continue
            imm = struct.unpack_from("<I", block, next_i)[0]
            label = None
            if imm <= 8:
                label = f"enum/u32 {imm}"
            elif imm in FLOAT_BITS:
                label = FLOAT_BITS[imm]
            if label is not None:
                hits.append((start_rva + start, disp, label, "mov dword [mem], imm32"))

    return sorted(set(hits))


def context_block(image: PeImage, center: int) -> tuple[int, bytes] | None:
    for s in image.sections:
        if not s.executable or not (s.rva <= center < s.rva + s.raw_size):
            continue
        start = max(s.rva, center - CONTEXT_BEFORE)
        end = min(s.rva + s.raw_size, center + CONTEXT_AFTER)
        try:
            return start, image.bytes_at(start, end - start)
        except ValueError:
            return None
    return None


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--exe", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()

    image = PeImage(args.exe.resolve())
    refs = executable_rip_refs(image)
    funcs = pdata_runtime_functions(image)

    by_target: defaultdict[int, list[tuple[int, int, str]]] = defaultdict(list)
    for ref in refs:
        by_target[ref[1]].append(ref)

    lines = [
        "TFM2 DIRECT CONTROL - SPEED STATE PROBE",
        f"Executable: {image.path}",
        f"SHA-256: {sha256(image.data)}",
        f"PE timestamp: 0x{image.timestamp:08X}",
        f"Image size: 0x{image.image_size:08X}",
        "",
        "Read-only static analysis. No game files or process memory were modified.",
        "",
    ]

    anchors: list[tuple[str, int]] = []

    for category, names in (("ACTION", ACTION_ANCHORS), ("UI", UI_ANCHORS)):
        lines.append(f"===== {category} ANCHORS =====")
        for name in names:
            lines.append(f"\n--- {name} ---")
            string_rvas = find_ascii_rvas(image, name)
            if not string_rvas:
                lines.append("string not found")
                continue

            for string_rva in string_rvas:
                lines.append(f"string RVA 0x{string_rva:08X}")
                direct = by_target.get(string_rva, [])
                ptrs = find_pointer_occurrences(image, string_rva)
                candidate_refs = list(direct)

                for ptr_rva in ptrs:
                    candidate_refs.extend(by_target.get(ptr_rva, []))
                    candidate_refs.extend(
                        ref for ref in refs
                        if abs(ref[1] - ptr_rva) <= NEAR_DESCRIPTOR_RADIUS
                    )

                dedup = sorted(set(candidate_refs))
                lines.append(
                    f"  descriptors={len(ptrs)} code_refs/direct_or_near={len(dedup)}"
                )
                for xref, target, kind in dedup[:40]:
                    owner = owning_function(funcs, xref)
                    owner_text = (
                        f"owner=0x{owner[0]:08X}..0x{owner[1]:08X}"
                        if owner
                        else "owner=?"
                    )
                    lines.append(
                        f"  REF 0x{xref:08X} {kind} -> 0x{target:08X} {owner_text}"
                    )
                    anchors.append((name, xref))

                    ctx = context_block(image, xref)
                    if ctx is None:
                        continue
                    start, block = ctx
                    writes = candidate_writes(block, start)
                    if writes:
                        lines.append("    nearby small/float field writes:")
                        for insn, disp, value, op in writes[:60]:
                            disp_text = "rip/absolute" if disp is None else f"{disp:+#x}"
                            lines.append(
                                f"      0x{insn:08X}: disp={disp_text} value={value} {op}"
                            )

    lines.append("\n===== UNIQUE OWNER FUNCTIONS =====")
    owners: defaultdict[tuple[int, int] | None, set[str]] = defaultdict(set)
    for name, xref in anchors:
        owners[owning_function(funcs, xref)].add(name)

    ranked = sorted(
        owners.items(),
        key=lambda item: (
            -len(item[1]),
            item[0][0] if item[0] else 0xFFFFFFFF,
        ),
    )
    for owner, names in ranked[:80]:
        if owner is None:
            lines.append(f"owner=? anchors={','.join(sorted(names))}")
            continue
        begin, end = owner
        lines.append(
            f"\nowner=0x{begin:08X}..0x{end:08X} size=0x{end-begin:X} "
            f"anchors={','.join(sorted(names))}"
        )
        try:
            block = image.bytes_at(begin, min(end - begin, 0x900))
        except ValueError:
            continue
        writes = candidate_writes(block, begin)
        for insn, disp, value, op in writes[:120]:
            disp_text = "rip/absolute" if disp is None else f"{disp:+#x}"
            lines.append(
                f"  WRITE 0x{insn:08X}: disp={disp_text} value={value} {op}"
            )
        lines.append("  first bytes:")
        lines.append(hexdump(block[: min(len(block), 0x180)], begin))

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text("\n".join(lines), encoding="utf-8")
    print(args.output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
