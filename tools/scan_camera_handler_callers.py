#!/usr/bin/env python3
"""Targeted v0.6.1 camera-handler caller probe.

This intentionally does *not* rescan semantic action ids. We already know the validated
camera handler RVA for v0.6.1 (0x00C2DBE0). This probe walks executable sections once for
rel32 CALL/JMP sites targeting that RVA, resolves each call site's .pdata owner, and dumps:
- the call-site neighborhood,
- the owning runtime function,
- direct rel32 callees from that owner,
- RIP-relative LEA/MOV references and nearby printable data.

The goal is to identify the upstream input/follow controller that invokes the camera handler.
"""

from __future__ import annotations

import argparse
import hashlib
import struct
from dataclasses import dataclass
from pathlib import Path

EXPECTED_TIMESTAMP = 0x6AB1D950
EXPECTED_IMAGE_SIZE = 0x05264000
CAMERA_HANDLER_RVA = 0x00C2DBE0


@dataclass(frozen=True)
class Section:
    name: str
    rva: int
    virtual_size: int
    raw_offset: int
    raw_size: int
    characteristics: int

    @property
    def executable(self) -> bool:
        return bool(self.characteristics & 0x20000000)


@dataclass(frozen=True)
class RuntimeFunction:
    start: int
    end: int


class PeImage:
    def __init__(self, path: Path):
        self.path = path
        self.data = path.read_bytes()
        if self.data[:2] != b"MZ":
            raise ValueError("not a PE image")
        pe = struct.unpack_from("<I", self.data, 0x3C)[0]
        if self.data[pe:pe+4] != b"PE\0\0":
            raise ValueError("invalid PE signature")

        coff = pe + 4
        machine, section_count, timestamp = struct.unpack_from("<HHI", self.data, coff)
        if machine != 0x8664:
            raise ValueError(f"expected x64 PE, got machine 0x{machine:04X}")
        self.timestamp = timestamp

        optional = coff + 20
        magic = struct.unpack_from("<H", self.data, optional)[0]
        if magic != 0x20B:
            raise ValueError(f"expected PE32+, got 0x{magic:04X}")
        self.image_size = struct.unpack_from("<I", self.data, optional + 56)[0]
        optional_size = struct.unpack_from("<H", self.data, coff + 16)[0]

        section_table = optional + optional_size
        sections = []
        for i in range(section_count):
            off = section_table + i * 40
            name = self.data[off:off+8].split(b"\0", 1)[0].decode("ascii", "replace")
            virtual_size, rva, raw_size, raw_offset = struct.unpack_from("<IIII", self.data, off + 8)
            characteristics = struct.unpack_from("<I", self.data, off + 36)[0]
            sections.append(Section(name, rva, virtual_size, raw_offset, raw_size, characteristics))
        self.sections = sections

    def rva_to_offset(self, rva: int) -> int:
        for s in self.sections:
            span = max(s.virtual_size, s.raw_size)
            if s.rva <= rva < s.rva + span:
                delta = rva - s.rva
                if delta >= s.raw_size:
                    raise ValueError(f"RVA 0x{rva:X} has no raw backing")
                return s.raw_offset + delta
        raise ValueError(f"RVA 0x{rva:X} not mapped")

    def bytes_at_rva(self, rva: int, size: int) -> bytes:
        off = self.rva_to_offset(rva)
        return self.data[off:off+size]

    def runtime_functions(self) -> list[RuntimeFunction]:
        pdata = next((s for s in self.sections if s.name == ".pdata"), None)
        if pdata is None:
            return []
        blob = self.data[pdata.raw_offset:pdata.raw_offset+pdata.raw_size]
        out = []
        for off in range(0, len(blob) - 11, 12):
            start, end, _unwind = struct.unpack_from("<III", blob, off)
            if start and end > start:
                out.append(RuntimeFunction(start, end))
        out.sort(key=lambda x: x.start)
        return out


def owner_of(functions: list[RuntimeFunction], rva: int) -> RuntimeFunction | None:
    lo, hi = 0, len(functions)
    while lo < hi:
        mid = (lo + hi) // 2
        fn = functions[mid]
        if rva < fn.start:
            hi = mid
        elif rva >= fn.end:
            lo = mid + 1
        else:
            return fn
    return None


def direct_edges(image: PeImage, target: int) -> list[tuple[int, str]]:
    out = []
    for sec in image.sections:
        if not sec.executable or sec.raw_size < 5:
            continue
        blob = image.data[sec.raw_offset:sec.raw_offset+sec.raw_size]
        base = sec.rva
        for i in range(len(blob) - 4):
            op = blob[i]
            if op not in (0xE8, 0xE9):
                continue
            disp = struct.unpack_from("<i", blob, i + 1)[0]
            site = base + i
            resolved = site + 5 + disp
            if resolved == target:
                out.append((site, "call" if op == 0xE8 else "jmp"))
    return out


def collect_rel32_calls(image: PeImage, start: int, end: int) -> list[tuple[int, int]]:
    try:
        blob = image.bytes_at_rva(start, end - start)
    except ValueError:
        return []
    out = []
    for i in range(len(blob) - 4):
        if blob[i] != 0xE8:
            continue
        disp = struct.unpack_from("<i", blob, i + 1)[0]
        site = start + i
        out.append((site, site + 5 + disp))
    return out


def collect_rip_refs(image: PeImage, start: int, end: int) -> list[tuple[int, int, str]]:
    try:
        blob = image.bytes_at_rva(start, end - start)
    except ValueError:
        return []
    out = []
    # Common x64 RIP-relative LEA/MOV forms used by Rust/MSVC.
    for i in range(len(blob) - 7):
        # [REX] 8D /r disp32 and [REX] 8B /r disp32 with modrm mod=00 r/m=101.
        if blob[i] in range(0x40, 0x50) and blob[i+1] in (0x8D, 0x8B):
            modrm = blob[i+2]
            if modrm & 0xC7 == 0x05:
                disp = struct.unpack_from("<i", blob, i + 3)[0]
                site = start + i
                target = site + 7 + disp
                out.append((site, target, "lea" if blob[i+1] == 0x8D else "mov"))
        elif blob[i] in (0x8D, 0x8B):
            modrm = blob[i+1]
            if modrm & 0xC7 == 0x05:
                disp = struct.unpack_from("<i", blob, i + 2)[0]
                site = start + i
                target = site + 6 + disp
                out.append((site, target, "lea" if blob[i] == 0x8D else "mov"))
    return out


def printable_at(image: PeImage, rva: int, limit: int = 96) -> str | None:
    try:
        raw = image.bytes_at_rva(rva, limit)
    except ValueError:
        return None
    run = bytearray()
    for b in raw:
        if b == 0:
            break
        if not (0x20 <= b <= 0x7E):
            break
        run.append(b)
    if len(run) < 4:
        return None
    return run.decode("ascii", "replace")


def emit_hex(lines: list[str], image: PeImage, title: str, start: int, end: int) -> None:
    lines.append(f"===== {title} =====")
    try:
        blob = image.bytes_at_rva(start, end - start)
    except ValueError as exc:
        lines.append(f"<unavailable: {exc}>")
        lines.append("")
        return

    for off in range(0, len(blob), 16):
        chunk = blob[off:off+16]
        hexpart = " ".join(f"{b:02X}" for b in chunk)
        ascii_part = "".join(chr(b) if 0x20 <= b <= 0x7E else "." for b in chunk)
        lines.append(f"    {start+off:08X}  {hexpart:<47}  {ascii_part}")
    lines.append("")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("exe", type=Path)
    ap.add_argument("--output", type=Path, required=True)
    args = ap.parse_args()

    image = PeImage(args.exe.resolve())
    digest = hashlib.sha256(image.data).hexdigest()
    lines = [
        "TFM2 Direct Control — targeted camera-handler caller probe",
        f"exe: {image.path}",
        f"sha256: {digest}",
        f"timestamp: 0x{image.timestamp:08X}",
        f"image_size: 0x{image.image_size:08X}",
        f"camera_handler_rva: 0x{CAMERA_HANDLER_RVA:08X}",
        "",
    ]

    if image.timestamp != EXPECTED_TIMESTAMP or image.image_size != EXPECTED_IMAGE_SIZE:
        lines.append("ERROR: executable is not the verified v0.6.1 build; refusing speculative decode.")
        args.output.write_text("\n".join(lines), encoding="utf-8")
        return 2

    funcs = image.runtime_functions()
    edges = direct_edges(image, CAMERA_HANDLER_RVA)
    lines.append(f"direct CALL/JMP edges to camera handler: {len(edges)}")
    lines.append("")

    if not edges:
        lines.append("No direct rel32 callers found. Next probe must inspect indirect references.")
        args.output.write_text("\n".join(lines), encoding="utf-8")
        return 0

    seen_owners = set()
    for site, kind in edges:
        owner = owner_of(funcs, site)
        lines.append(
            f"{kind.upper()} site 0x{site:08X} -> 0x{CAMERA_HANDLER_RVA:08X}; "
            + (
                f"owner 0x{owner.start:08X}..0x{owner.end:08X}"
                if owner else "owner <unknown>"
            )
        )
        emit_hex(
            lines, image, f"CAMERA CALLSITE 0x{site:08X}",
            max(owner.start if owner else 0, site - 0x180),
            min(owner.end if owner else image.image_size, site + 0x220),
        )

        if owner is None or (owner.start, owner.end) in seen_owners:
            continue
        seen_owners.add((owner.start, owner.end))
        span = owner.end - owner.start
        lines.append(
            f"OWNER SUMMARY 0x{owner.start:08X}..0x{owner.end:08X} size=0x{span:X}"
        )
        for callsite, target in collect_rel32_calls(image, owner.start, owner.end):
            suffix = " <camera handler>" if target == CAMERA_HANDLER_RVA else ""
            lines.append(f"  call 0x{callsite:08X} -> 0x{target:08X}{suffix}")

        refs = collect_rip_refs(image, owner.start, owner.end)
        for xref, target, kind2 in refs:
            text = printable_at(image, target)
            if text:
                lines.append(
                    f"  {kind2} 0x{xref:08X} -> 0x{target:08X}: {text!r}"
                )
        lines.append("")

        if span <= 0x1800:
            emit_hex(lines, image, f"CAMERA CALLER OWNER 0x{owner.start:08X}", owner.start, owner.end)

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text("\n".join(lines), encoding="utf-8")
    print(f"Wrote {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
