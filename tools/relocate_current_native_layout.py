#!/usr/bin/env python3
"""Read-only native-target relocation probe for Teamfight Manager 2 updates.

This revision uses the PE's x64 unwind table (.pdata) so we only treat real
function starts as relocation candidates. That avoids the huge false-positive
set produced by scanning for a common Rust prologue anywhere in .text.

Known Direct Control invariants from v0.5.8 and physically validated v0.6.0:
- the three watched/client simulation jobs use the same 12-byte prologue;
- A/B/C were exactly 0xC10 apart in both builds;
- all three call one common, comparatively large simulation wrapper;
- the spectator camera handler uses the same 12-byte prologue;
- v0.6.0 camera fields were zoom +E0, center +E4/+E8, extents +EC/+F0,
  mode +100, pan +428/+42C.

No game files or process memory are modified.
"""

from __future__ import annotations

import argparse
import bisect
import hashlib
import struct
from dataclasses import dataclass
from pathlib import Path


V060_CAMERA_RVA = 0x009C_EBF0
V060_CANDIDATES = (0x00AC_2AE0, 0x00AC_36F0, 0x00AC_4300)
V060_WRAPPER_RVA = 0x016D_2740
V060_RUNNER_ANCHOR_RVA = 0x016D_3880

EXPECTED_PROLOGUE = bytes.fromhex(
    "55 41 57 41 56 41 55 41 54 56 57 53"
)
SIM_STRIDE = 0xC10
SIM_SCAN_BYTES = 0x300

CAMERA_SIGNATURES = {
    "zoom +0xE0": bytes.fromhex("F3 0F 10 86 E0 00 00 00"),
    "center pair +0xE4": bytes.fromhex("F2 0F 10 86 E4 00 00 00"),
    "mode +0x100": bytes.fromhex("44 0F B6 A6 00 01 00 00"),
    "pan vector +0x428": bytes.fromhex("48 8D 86 28 04 00 00"),
    "pan X write +0x428": bytes.fromhex("C7 86 28 04 00 00"),
    "pan Y write +0x42C": bytes.fromhex("C7 86 2C 04 00 00"),
}

GENERIC_CAMERA_FORMS = {
    "movss [rsi+disp32]": bytes.fromhex("F3 0F 10 86"),
    "movsd [rsi+disp32]": bytes.fromhex("F2 0F 10 86"),
    "movzx r12d,[rsi+disp32]": bytes.fromhex("44 0F B6 A6"),
    "lea rax,[rsi+disp32]": bytes.fromhex("48 8D 86"),
    "mov dword [rsi+disp32],imm32": bytes.fromhex("C7 86"),
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


@dataclass(frozen=True)
class RuntimeFunction:
    begin: int
    end: int
    unwind: int

    @property
    def size(self) -> int:
        return self.end - self.begin


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
        self.runtime_functions = self._load_runtime_functions()
        self._runtime_begins = [fn.begin for fn in self.runtime_functions]

    def rva_to_offset(self, rva: int) -> int:
        for section in self.sections:
            if section.rva <= rva < section.rva + section.span:
                delta = rva - section.rva
                if delta >= section.raw_size:
                    raise ValueError(f"RVA 0x{rva:X} is in zero-filled section tail")
                return section.raw_offset + delta
        raise ValueError(f"RVA 0x{rva:X} is outside mapped sections")

    def bytes_at(self, rva: int, size: int) -> bytes:
        offset = self.rva_to_offset(rva)
        return self.data[offset : offset + size]

    def is_executable_rva(self, rva: int) -> bool:
        return any(
            section.executable and section.rva <= rva < section.rva + section.span
            for section in self.sections
        )

    def _load_runtime_functions(self) -> list[RuntimeFunction]:
        pdata = next((s for s in self.sections if s.name == ".pdata"), None)
        if pdata is None:
            return []

        block = self.data[pdata.raw_offset : pdata.raw_offset + pdata.raw_size]
        out: list[RuntimeFunction] = []
        for offset in range(0, len(block) - 11, 12):
            begin, end, unwind = struct.unpack_from("<III", block, offset)
            if begin == 0 and end == 0 and unwind == 0:
                continue
            if begin >= end or not self.is_executable_rva(begin):
                continue
            out.append(RuntimeFunction(begin, end, unwind))

        # Rust/LLVM can emit duplicate/chained entries. Keep the widest entry per begin.
        by_begin: dict[int, RuntimeFunction] = {}
        for fn in out:
            prior = by_begin.get(fn.begin)
            if prior is None or fn.end > prior.end:
                by_begin[fn.begin] = fn
        return sorted(by_begin.values(), key=lambda fn: fn.begin)

    def runtime_function_at(self, rva: int) -> RuntimeFunction | None:
        if not self.runtime_functions:
            return None
        index = bisect.bisect_right(self._runtime_begins, rva) - 1
        if index < 0:
            return None
        fn = self.runtime_functions[index]
        return fn if fn.begin <= rva < fn.end else None


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def fmt_bytes(data: bytes | None) -> str:
    return "<unmapped>" if data is None else " ".join(f"{byte:02X}" for byte in data)


def safe_bytes(image: PeImage, rva: int, size: int) -> bytes | None:
    try:
        data = image.bytes_at(rva, size)
    except ValueError:
        return None
    return data if len(data) == size else None


def function_has_prologue(image: PeImage, fn: RuntimeFunction) -> bool:
    return safe_bytes(image, fn.begin, len(EXPECTED_PROLOGUE)) == EXPECTED_PROLOGUE


def rel32_calls(
    image: PeImage, start_rva: int, size: int
) -> list[tuple[int, int]]:
    block = safe_bytes(image, start_rva, size)
    if block is None:
        return []
    out: list[tuple[int, int]] = []
    for index in range(len(block) - 4):
        if block[index] != 0xE8:
            continue
        displacement = struct.unpack_from("<i", block, index + 1)[0]
        call_rva = start_rva + index
        target_rva = call_rva + 5 + displacement
        if image.is_executable_rva(target_rva):
            out.append((call_rva, target_rva))
    return out


def call_target_counts(
    image: PeImage, start_rva: int, size: int
) -> dict[int, int]:
    counts: dict[int, int] = {}
    for _, target in rel32_calls(image, start_rva, size):
        counts[target] = counts.get(target, 0) + 1
    return counts


def nearest_old_delta(new_rva: int, old_rva: int) -> str:
    delta = new_rva - old_rva
    sign = "+" if delta >= 0 else "-"
    return f"{sign}0x{abs(delta):X}"


def find_executable_occurrences(image: PeImage, needle: bytes) -> list[int]:
    out: list[int] = []
    for section in image.sections:
        if not section.executable or section.raw_size < len(needle):
            continue
        block = image.data[
            section.raw_offset : section.raw_offset + section.raw_size
        ]
        start = 0
        while True:
            index = block.find(needle, start)
            if index < 0:
                break
            out.append(section.rva + index)
            start = index + 1
    return out


def simulation_candidates(
    image: PeImage,
) -> list[tuple[tuple[RuntimeFunction, RuntimeFunction, RuntimeFunction], list[int]]]:
    starts = {
        fn.begin: fn
        for fn in image.runtime_functions
        if function_has_prologue(image, fn)
    }
    out = []

    for a_rva, a in sorted(starts.items()):
        b = starts.get(a_rva + SIM_STRIDE)
        c = starts.get(a_rva + 2 * SIM_STRIDE)
        if b is None or c is None:
            continue

        maps = [
            call_target_counts(image, a.begin, min(a.size, SIM_SCAN_BYTES)),
            call_target_counts(image, b.begin, min(b.size, SIM_SCAN_BYTES)),
            call_target_counts(image, c.begin, min(c.size, SIM_SCAN_BYTES)),
        ]
        common = sorted(set(maps[0]) & set(maps[1]) & set(maps[2]))
        if common:
            out.append(((a, b, c), common))

    def score(item):
        (a, b, c), common = item
        # Prefer triples whose functions nearly fill their 0xC10 slots, as the
        # validated jobs did, and whose common target includes a large function.
        fill = a.size + b.size + c.size
        largest_target = max(
            (
                image.runtime_function_at(target).size
                if image.runtime_function_at(target) is not None
                else 0
            )
            for target in common
        )
        proximity_penalty = abs(a.begin - V060_CANDIDATES[0])
        return (-largest_target, -fill, proximity_penalty, a.begin)

    out.sort(key=score)
    return out


def camera_function_scores(
    image: PeImage,
) -> list[tuple[int, RuntimeFunction, dict[str, list[int]]]]:
    grouped: dict[int, dict[str, list[int]]] = {}

    for label, signature in CAMERA_SIGNATURES.items():
        for occurrence in find_executable_occurrences(image, signature):
            fn = image.runtime_function_at(occurrence)
            if fn is None or not function_has_prologue(image, fn):
                continue
            grouped.setdefault(fn.begin, {}).setdefault(label, []).append(
                occurrence - fn.begin
            )

    scored: list[tuple[int, RuntimeFunction, dict[str, list[int]]]] = []
    functions = {fn.begin: fn for fn in image.runtime_functions}
    for begin, hits in grouped.items():
        fn = functions[begin]
        scored.append((len(hits), fn, hits))

    scored.sort(
        key=lambda item: (
            -item[0],
            abs(item[1].begin - V060_CAMERA_RVA),
            -item[1].size,
        )
    )
    return scored


def generic_disp_hits(block: bytes, prefix: bytes) -> list[tuple[int, int]]:
    out: list[tuple[int, int]] = []
    start = 0
    while True:
        index = block.find(prefix, start)
        if index < 0:
            break
        disp_at = index + len(prefix)
        if disp_at + 4 <= len(block):
            disp = struct.unpack_from("<i", block, disp_at)[0]
            if 0 <= disp <= 0x1000:
                out.append((index, disp))
        start = index + 1
    return out


def emit_camera_generic(
    lines: list[str], image: PeImage, fn: RuntimeFunction
) -> None:
    body = safe_bytes(image, fn.begin, fn.size)
    if body is None:
        return
    lines.append("  generic decoded camera-like field accesses:")
    any_hit = False
    for label, prefix in GENERIC_CAMERA_FORMS.items():
        hits = generic_disp_hits(body, prefix)
        if not hits:
            continue
        any_hit = True
        rendered = ", ".join(
            f"+0x{where:X}->disp 0x{disp:X}" for where, disp in hits[:32]
        )
        lines.append(f"    {label}: {rendered}")
    if not any_hit:
        lines.append("    <none>")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("exe", type=Path, help="path to current TeamfightManager2.exe")
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("tfm2-native-relocation.txt"),
        help="report path (default: tfm2-native-relocation.txt)",
    )
    args = parser.parse_args()

    image = PeImage(args.exe.resolve())
    digest = sha256(image.data)
    prologue_functions = [
        fn for fn in image.runtime_functions if function_has_prologue(image, fn)
    ]

    lines: list[str] = [
        "TFM2 DIRECT CONTROL - NATIVE TARGET RELOCATION REPORT (PDATA)",
        f"Executable: {image.path}",
        f"SHA-256: {digest}",
        f"PE timestamp: 0x{image.timestamp:08X}",
        f"Image size: 0x{image.image_size:08X}",
        f"Image base: 0x{image.image_base:X}",
        f"Runtime functions from .pdata: {len(image.runtime_functions)}",
        f"Runtime function starts with Direct-Control prologue: {len(prologue_functions)}",
        "",
        "Read-only static analysis. No game files or process memory were modified.",
        "",
        "===== SIMULATION JOB RELOCATION =====",
        (
            "Only real .pdata function starts are considered. Historical invariant: "
            "A/B/C starts are exactly 0xC10 apart and all call one common wrapper."
        ),
    ]

    sim = simulation_candidates(image)
    if not sim:
        lines.append("SIMULATION RESULT: <no strict real-function triple found>")
    else:
        for index, (triple, common) in enumerate(sim[:20], 1):
            a, b, c = triple
            lines.append("")
            lines.append(
                f"SIM #{index}: "
                f"A=0x{a.begin:08X} B=0x{b.begin:08X} C=0x{c.begin:08X}"
            )
            lines.append(
                f"  function sizes A/B/C = "
                f"0x{a.size:X}/0x{b.size:X}/0x{c.size:X}"
            )
            lines.append(
                f"  deltas from v0.6.0 = "
                f"{nearest_old_delta(a.begin, V060_CANDIDATES[0])}/"
                f"{nearest_old_delta(b.begin, V060_CANDIDATES[1])}/"
                f"{nearest_old_delta(c.begin, V060_CANDIDATES[2])}"
            )
            lines.append(
                "  first 48 bytes A: "
                + fmt_bytes(safe_bytes(image, a.begin, 48))
            )
            lines.append("  common rel32 targets:")
            ranked_targets = []
            for target in common:
                target_fn = image.runtime_function_at(target)
                size = target_fn.size if target_fn is not None else 0
                ranked_targets.append((size, target, target_fn))
            ranked_targets.sort(reverse=True)

            for size, target, target_fn in ranked_targets:
                counts = [
                    call_target_counts(image, fn.begin, min(fn.size, SIM_SCAN_BYTES)).get(
                        target, 0
                    )
                    for fn in triple
                ]
                owner = (
                    f"fn 0x{target_fn.begin:08X}..0x{target_fn.end:08X}"
                    if target_fn is not None
                    else "<no pdata owner>"
                )
                lines.append(
                    f"    0x{target:08X} calls={counts[0]}/{counts[1]}/{counts[2]} "
                    f"owner={owner} size=0x{size:X} "
                    f"delta-old-wrapper={nearest_old_delta(target, V060_WRAPPER_RVA)}"
                )
                lines.append(
                    "      bytes: " + fmt_bytes(safe_bytes(image, target, 48))
                )

    lines.extend(
        [
            "",
            "===== CAMERA HANDLER RELOCATION =====",
            (
                "Exact camera signatures are first located globally, then attributed to "
                "their real .pdata owning function. This removes overlapping-window "
                "false positives from the previous probe."
            ),
        ]
    )

    camera = camera_function_scores(image)
    if not camera:
        lines.append("CAMERA RESULT: <no prologue-matching owner for known signatures>")
    else:
        for index, (score, fn, hits) in enumerate(camera[:12], 1):
            lines.append("")
            lines.append(
                f"CAMERA #{index}: score={score}/6 "
                f"RVA=0x{fn.begin:08X} end=0x{fn.end:08X} size=0x{fn.size:X} "
                f"delta-old={nearest_old_delta(fn.begin, V060_CAMERA_RVA)}"
            )
            for label in CAMERA_SIGNATURES:
                offsets = hits.get(label)
                if offsets:
                    lines.append(
                        f"  PASS {label:24s} @ "
                        + ", ".join(f"+0x{offset:X}" for offset in offsets[:8])
                    )
                else:
                    lines.append(f"  ---- {label}")
            lines.append(
                "  first 48 bytes: " + fmt_bytes(safe_bytes(image, fn.begin, 48))
            )
            if index <= 3:
                emit_camera_generic(lines, image, fn)

    lines.extend(
        [
            "",
            "===== INTERPRETATION =====",
            (
                "A simulation result is strong when one real-function triple dominates "
                "and one common call target owns a much larger function than incidental "
                "helpers. A camera result is strong when one real owning function contains "
                "five or six of the known field signatures. Missing mode +0x100 alone may "
                "mean that field moved or its instruction encoding changed; do not guess it."
            ),
        ]
    )

    output = args.output.resolve()
    output.write_text("\n".join(lines), encoding="utf-8")
    print(output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
