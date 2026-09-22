#!/usr/bin/env python3
"""Read-only native-target relocation probe for Teamfight Manager 2 updates.

This is the second-stage probe used when a new game build no longer matches the
physically validated v0.6.0 RVAs.

It searches the current executable for two structural invariants that survived
the v0.5.8 -> v0.6.0 migration:

1. The three client simulation jobs share the same 12-byte whole-instruction
   prologue and were laid out exactly 0xC10 bytes apart in both known builds.
   Each job calls the same game-core simulation wrapper.

2. The spectator-camera handler shares that prologue and contains a distinctive
   cluster of camera-field instructions. The first pass looks for the exact
   v0.6.0 field layout; a fallback reports decoded displacement candidates if
   the object layout changed again.

No game files or process memory are modified. This script only reads the PE.
"""

from __future__ import annotations

import argparse
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
CAMERA_SCAN_BYTES = 0x2200

CAMERA_SIGNATURES = {
    "zoom +0xE0": bytes.fromhex("F3 0F 10 86 E0 00 00 00"),
    "center pair +0xE4": bytes.fromhex("F2 0F 10 86 E4 00 00 00"),
    "mode +0x100": bytes.fromhex("44 0F B6 A6 00 01 00 00"),
    "pan vector +0x428": bytes.fromhex("48 8D 86 28 04 00 00"),
    "pan X write +0x428": bytes.fromhex("C7 86 28 04 00 00"),
    "pan Y write +0x42C": bytes.fromhex("C7 86 2C 04 00 00"),
}

# Generic forms used only as fallback diagnostics when the exact v0.6.0 layout
# no longer matches. Offsets are decoded from disp32.
CAMERA_GENERIC_FORMS = {
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

    def rva_to_offset(self, rva: int) -> int:
        for section in self.sections:
            if section.rva <= rva < section.rva + section.span:
                delta = rva - section.rva
                if delta >= section.raw_size:
                    raise ValueError(f"RVA 0x{rva:X} is in zero-filled section tail")
                return section.raw_offset + delta
        raise ValueError(f"RVA 0x{rva:X} is outside mapped sections")

    def offset_to_rva(self, offset: int) -> int:
        for section in self.sections:
            if section.raw_offset <= offset < section.raw_offset + section.raw_size:
                return section.rva + (offset - section.raw_offset)
        raise ValueError(f"file offset 0x{offset:X} is outside mapped sections")

    def bytes_at(self, rva: int, size: int) -> bytes:
        offset = self.rva_to_offset(rva)
        return self.data[offset : offset + size]

    def is_executable_rva(self, rva: int) -> bool:
        return any(
            section.executable and section.rva <= rva < section.rva + section.span
            for section in self.sections
        )


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def fmt_bytes(data: bytes | None) -> str:
    if data is None:
        return "<unmapped>"
    return " ".join(f"{byte:02X}" for byte in data)


def safe_bytes(image: PeImage, rva: int, size: int) -> bytes | None:
    try:
        data = image.bytes_at(rva, size)
    except ValueError:
        return None
    return data if len(data) == size else None


def find_all_executable(image: PeImage, needle: bytes) -> list[int]:
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
    return sorted(out)


def rel32_calls(image: PeImage, start_rva: int, size: int) -> list[tuple[int, int]]:
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


def strict_simulation_triples(
    image: PeImage, prologues: list[int]
) -> list[tuple[tuple[int, int, int], list[int]]]:
    prologue_set = set(prologues)
    out: list[tuple[tuple[int, int, int], list[int]]] = []

    for a in prologues:
        b = a + SIM_STRIDE
        c = b + SIM_STRIDE
        if b not in prologue_set or c not in prologue_set:
            continue

        maps = [
            call_target_counts(image, a, SIM_SCAN_BYTES),
            call_target_counts(image, b, SIM_SCAN_BYTES),
            call_target_counts(image, c, SIM_SCAN_BYTES),
        ]
        common = sorted(set(maps[0]) & set(maps[1]) & set(maps[2]))
        if common:
            out.append(((a, b, c), common))

    return out


def fuzzy_simulation_triples(
    image: PeImage, prologues: list[int]
) -> list[tuple[int, tuple[int, int, int], list[int]]]:
    """Fallback if the exact 0xC10 spacing changed.

    Search nearby prologue triples with roughly the historical spacing and require
    at least one common executable rel32 target from all three functions.
    """

    out: list[tuple[int, tuple[int, int, int], list[int]]] = []
    calls_cache: dict[int, dict[int, int]] = {}

    def targets(rva: int) -> dict[int, int]:
        if rva not in calls_cache:
            calls_cache[rva] = call_target_counts(image, rva, SIM_SCAN_BYTES)
        return calls_cache[rva]

    for ai, a in enumerate(prologues):
        b_candidates = [
            b
            for b in prologues[ai + 1 :]
            if 0xA80 <= b - a <= 0xDA0
        ]
        for b in b_candidates:
            c_candidates = [
                c
                for c in prologues
                if c > b and 0xA80 <= c - b <= 0xDA0
            ]
            for c in c_candidates:
                common = sorted(
                    set(targets(a)) & set(targets(b)) & set(targets(c))
                )
                if not common:
                    continue
                penalty = abs((b - a) - SIM_STRIDE) + abs((c - b) - SIM_STRIDE)
                out.append((penalty, (a, b, c), common))

    out.sort(key=lambda item: (item[0], item[1]))
    return out[:20]


def camera_exact_scores(
    image: PeImage, prologues: list[int]
) -> list[tuple[int, int, dict[str, int]]]:
    out: list[tuple[int, int, dict[str, int]]] = []
    for rva in prologues:
        body = safe_bytes(image, rva, CAMERA_SCAN_BYTES)
        if body is None:
            continue
        hits: dict[str, int] = {}
        for label, signature in CAMERA_SIGNATURES.items():
            where = body.find(signature)
            if where >= 0:
                hits[label] = where
        if hits:
            out.append((len(hits), rva, hits))
    out.sort(key=lambda item: (-item[0], abs(item[1] - V060_CAMERA_RVA)))
    return out


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
            # Camera object fields are small positive offsets. This bound keeps the
            # fallback report readable without asserting a particular layout.
            if 0 <= disp <= 0x1000:
                out.append((index, disp))
        start = index + 1
    return out


def camera_generic_scores(
    image: PeImage, prologues: list[int]
) -> list[tuple[int, int, dict[str, list[tuple[int, int]]]]]:
    out: list[tuple[int, int, dict[str, list[tuple[int, int]]]]] = []
    for rva in prologues:
        body = safe_bytes(image, rva, CAMERA_SCAN_BYTES)
        if body is None:
            continue

        decoded: dict[str, list[tuple[int, int]]] = {}
        nonempty = 0
        total_hits = 0
        for label, prefix in CAMERA_GENERIC_FORMS.items():
            hits = generic_disp_hits(body, prefix)
            if hits:
                decoded[label] = hits[:24]
                nonempty += 1
                total_hits += min(len(hits), 24)

        # Requiring several different instruction forms filters generic large
        # Rust functions that happen to touch a few [rsi+disp] fields.
        if nonempty >= 4:
            score = nonempty * 100 + min(total_hits, 99)
            out.append((score, rva, decoded))

    out.sort(key=lambda item: (-item[0], abs(item[1] - V060_CAMERA_RVA)))
    return out[:20]


def nearest_old_delta(new_rva: int, old_rva: int) -> str:
    delta = new_rva - old_rva
    sign = "+" if delta >= 0 else "-"
    return f"{sign}0x{abs(delta):X}"


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
    prologues = find_all_executable(image, EXPECTED_PROLOGUE)

    lines: list[str] = [
        "TFM2 DIRECT CONTROL - NATIVE TARGET RELOCATION REPORT",
        f"Executable: {image.path}",
        f"SHA-256: {digest}",
        f"PE timestamp: 0x{image.timestamp:08X}",
        f"Image size: 0x{image.image_size:08X}",
        f"Image base: 0x{image.image_base:X}",
        f"Matching 12-byte function prologues: {len(prologues)}",
        "",
        "Read-only static analysis. No game files or process memory were modified.",
        "",
        "===== SIMULATION JOB RELOCATION =====",
        (
            "Historical invariant: A/B/C were exactly 0xC10 apart in both v0.5.8 "
            "and v0.6.0, and all called one common simulation wrapper."
        ),
    ]

    strict = strict_simulation_triples(image, prologues)
    if strict:
        for index, (triple, common) in enumerate(strict[:20], 1):
            a, b, c = triple
            lines.append("")
            lines.append(
                f"STRICT CANDIDATE #{index}: "
                f"A=0x{a:08X} B=0x{b:08X} C=0x{c:08X}"
            )
            lines.append(
                f"  deltas from v0.6.0: "
                f"A {nearest_old_delta(a, V060_CANDIDATES[0])}, "
                f"B {nearest_old_delta(b, V060_CANDIDATES[1])}, "
                f"C {nearest_old_delta(c, V060_CANDIDATES[2])}"
            )
            lines.append(
                "  first 32 bytes A: "
                + fmt_bytes(safe_bytes(image, a, 32))
            )
            lines.append("  common executable rel32 call targets:")
            for target in common:
                counts = [
                    call_target_counts(image, rva, SIM_SCAN_BYTES).get(target, 0)
                    for rva in triple
                ]
                lines.append(
                    f"    0x{target:08X} "
                    f"(calls A/B/C={counts[0]}/{counts[1]}/{counts[2]}, "
                    f"delta from old wrapper {nearest_old_delta(target, V060_WRAPPER_RVA)})"
                )
                lines.append(
                    "      target bytes: "
                    + fmt_bytes(safe_bytes(image, target, 32))
                )
    else:
        lines.append("")
        lines.append("No strict 0xC10 triple with a common call target was found.")
        lines.append("Trying fuzzy historical spacing...")
        fuzzy = fuzzy_simulation_triples(image, prologues)
        if not fuzzy:
            lines.append("FUZZY RESULT: <none>")
        else:
            for index, (penalty, triple, common) in enumerate(fuzzy[:10], 1):
                a, b, c = triple
                lines.append(
                    f"FUZZY #{index}: penalty=0x{penalty:X} "
                    f"A=0x{a:08X} B=0x{b:08X} C=0x{c:08X} "
                    f"gaps=0x{b-a:X}/0x{c-b:X}"
                )
                lines.append(
                    "  common targets: "
                    + ", ".join(f"0x{target:08X}" for target in common)
                )

    lines.extend(
        [
            "",
            "===== CAMERA HANDLER RELOCATION =====",
            "First rank: exact v0.6.0 camera-field signatures inside each matching prologue.",
        ]
    )

    exact_camera = camera_exact_scores(image, prologues)
    if not exact_camera:
        lines.append("Exact-layout candidates: <none>")
    else:
        for index, (score, rva, hits) in enumerate(exact_camera[:20], 1):
            lines.append("")
            lines.append(
                f"CAMERA EXACT #{index}: score={score}/6 RVA=0x{rva:08X} "
                f"(delta from v0.6.0 {nearest_old_delta(rva, V060_CAMERA_RVA)})"
            )
            for label in CAMERA_SIGNATURES:
                if label in hits:
                    lines.append(f"  PASS {label:24s} @ +0x{hits[label]:X}")
                else:
                    lines.append(f"  ---- {label}")
            lines.append(
                "  first 32 bytes: "
                + fmt_bytes(safe_bytes(image, rva, 32))
            )

    if not exact_camera or exact_camera[0][0] < len(CAMERA_SIGNATURES):
        lines.extend(
            [
                "",
                "===== CAMERA GENERIC DISPLACEMENT FALLBACK =====",
                (
                    "The exact old field cluster was not uniquely complete. "
                    "These are diagnostic candidates only; do not patch from them blindly."
                ),
            ]
        )
        generic_camera = camera_generic_scores(image, prologues)
        if not generic_camera:
            lines.append("<none>")
        else:
            for index, (score, rva, decoded) in enumerate(generic_camera[:10], 1):
                lines.append("")
                lines.append(
                    f"CAMERA GENERIC #{index}: score={score} RVA=0x{rva:08X} "
                    f"(delta from v0.6.0 {nearest_old_delta(rva, V060_CAMERA_RVA)})"
                )
                for label, hits in decoded.items():
                    rendered = ", ".join(
                        f"+0x{where:X}->disp 0x{disp:X}" for where, disp in hits
                    )
                    lines.append(f"  {label}: {rendered}")

    lines.extend(
        [
            "",
            "===== CROSS-CHECK =====",
            (
                "If the top simulation triple and top complete camera candidate moved by "
                "the same or very similar RVA delta, that is additional evidence of a "
                "layout-preserving code relocation. It is not a substitute for runtime validation."
            ),
        ]
    )

    if strict and exact_camera:
        sim_delta = strict[0][0][0] - V060_CANDIDATES[0]
        cam_delta = exact_camera[0][1] - V060_CAMERA_RVA
        lines.append(f"Top simulation A delta: {sim_delta:+#x}")
        lines.append(f"Top camera delta:       {cam_delta:+#x}")
        lines.append(
            "Delta agreement: "
            + ("EXACT" if sim_delta == cam_delta else "different")
        )

    output = args.output.resolve()
    output.write_text("\n".join(lines), encoding="utf-8")
    print(output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
