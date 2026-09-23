#!/usr/bin/env python3
"""Dump the two helpers used by the v0.6.1 game/view action-state loop.

The camera caller at 0x0086B920 iterates 55 semantic action ids:
    value = sub_021DBD30(action_id)
    sub_00CEB890(state, action_id, value)

This probe only dumps the .pdata owners around those two fixed RVAs. It performs no
whole-image brute-force scan and should complete essentially immediately.
"""

from __future__ import annotations

import argparse
import hashlib
import struct
from dataclasses import dataclass
from pathlib import Path

EXPECTED_TIMESTAMP = 0x6AB1D950
EXPECTED_IMAGE_SIZE = 0x05264000
TARGETS = (
    ("action_value_helper", 0x021DBD30),
    ("action_state_sink", 0x00CEB890),
)


@dataclass(frozen=True)
class Section:
    name: str
    rva: int
    virtual_size: int
    raw_offset: int
    raw_size: int


@dataclass(frozen=True)
class RuntimeFunction:
    start: int
    end: int


class PeImage:
    def __init__(self, path: Path):
        self.path = path
        self.data = path.read_bytes()
        pe = struct.unpack_from("<I", self.data, 0x3C)[0]
        if self.data[:2] != b"MZ" or self.data[pe:pe+4] != b"PE\0\0":
            raise ValueError("invalid PE image")
        coff = pe + 4
        machine, count, timestamp = struct.unpack_from("<HHI", self.data, coff)
        if machine != 0x8664:
            raise ValueError("expected x64 PE")
        self.timestamp = timestamp
        opt = coff + 20
        if struct.unpack_from("<H", self.data, opt)[0] != 0x20B:
            raise ValueError("expected PE32+")
        self.image_size = struct.unpack_from("<I", self.data, opt + 56)[0]
        opt_size = struct.unpack_from("<H", self.data, coff + 16)[0]
        table = opt + opt_size
        self.sections = []
        for i in range(count):
            off = table + i * 40
            name = self.data[off:off+8].split(b"\0",1)[0].decode("ascii","replace")
            vsize, rva, raw_size, raw_off = struct.unpack_from("<IIII", self.data, off+8)
            self.sections.append(Section(name, rva, vsize, raw_off, raw_size))

    def rva_to_offset(self, rva: int) -> int:
        for sec in self.sections:
            if sec.rva <= rva < sec.rva + max(sec.virtual_size, sec.raw_size):
                delta = rva - sec.rva
                if delta >= sec.raw_size:
                    raise ValueError(f"RVA 0x{rva:X} has no raw bytes")
                return sec.raw_offset + delta
        raise ValueError(f"unmapped RVA 0x{rva:X}")

    def bytes_at(self, rva: int, size: int) -> bytes:
        off = self.rva_to_offset(rva)
        return self.data[off:off+size]

    def runtime_functions(self) -> list[RuntimeFunction]:
        pdata = next((s for s in self.sections if s.name == ".pdata"), None)
        if pdata is None:
            return []
        blob = self.data[pdata.raw_offset:pdata.raw_offset+pdata.raw_size]
        out = []
        for off in range(0, len(blob)-11, 12):
            start, end, _ = struct.unpack_from("<III", blob, off)
            if start and end > start:
                out.append(RuntimeFunction(start,end))
        out.sort(key=lambda f: f.start)
        return out


def owner_of(funcs: list[RuntimeFunction], rva: int) -> RuntimeFunction | None:
    lo, hi = 0, len(funcs)
    while lo < hi:
        mid = (lo+hi)//2
        fn = funcs[mid]
        if rva < fn.start:
            hi = mid
        elif rva >= fn.end:
            lo = mid+1
        else:
            return fn
    return None


def emit(lines: list[str], image: PeImage, title: str, start: int, end: int) -> None:
    lines.append(f"===== {title} =====")
    blob = image.bytes_at(start, end-start)
    for off in range(0,len(blob),16):
        chunk = blob[off:off+16]
        hx = " ".join(f"{b:02X}" for b in chunk)
        asc = "".join(chr(b) if 0x20 <= b <= 0x7E else "." for b in chunk)
        lines.append(f"{start+off:08X}  {hx:<47}  {asc}")
    lines.append("")


def main() -> int:
    ap=argparse.ArgumentParser()
    ap.add_argument("exe",type=Path)
    ap.add_argument("--output",type=Path,required=True)
    args=ap.parse_args()
    img=PeImage(args.exe.resolve())
    lines=[
        "TFM2 Direct Control — action helper probe",
        f"exe: {img.path}",
        f"sha256: {hashlib.sha256(img.data).hexdigest()}",
        f"timestamp: 0x{img.timestamp:08X}",
        f"image_size: 0x{img.image_size:08X}",
        "",
    ]
    if img.timestamp != EXPECTED_TIMESTAMP or img.image_size != EXPECTED_IMAGE_SIZE:
        lines.append("ERROR: executable is not verified v0.6.1; refusing decode.")
        args.output.write_text("\n".join(lines),encoding="utf-8")
        return 2

    funcs=img.runtime_functions()
    for name,rva in TARGETS:
        owner=owner_of(funcs,rva)
        lines.append(f"{name}: RVA 0x{rva:08X}")
        if owner:
            lines.append(f"owner: 0x{owner.start:08X}..0x{owner.end:08X} size=0x{owner.end-owner.start:X}")
            # Full owner when modest; otherwise first 0x400 around target.
            if owner.end-owner.start <= 0x1200:
                start,end=owner.start,owner.end
            else:
                start=max(owner.start,rva-0x180)
                end=min(owner.end,rva+0x500)
        else:
            lines.append("owner: <none>")
            start=max(0,rva-0x80)
            end=rva+0x300
        emit(lines,img,f"{name} bytes",start,end)

    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text("\n".join(lines),encoding="utf-8")
    print(f"Wrote {args.output}")
    return 0


if __name__=="__main__":
    raise SystemExit(main())
