#!/usr/bin/env python3
"""Read-only compatibility triage for a newly updated Teamfight Manager 2 build.

This does NOT assume the new build is compatible. It fingerprints the executable
and checks whether Direct Control's physically validated v0.6.0 native targets
still have the same machine-level shape at the same RVAs.

If every same-RVA check passes, the update may only require adding a new guarded
build fingerprint. If any check fails, do not reuse the old profile blindly:
relocate/revalidate the affected native target first.

No game files or process memory are modified.
"""

from __future__ import annotations

import argparse
import hashlib
import struct
from dataclasses import dataclass
from pathlib import Path


V060_SHA256 = "aecb984a2c7ae187092a399b725d30db6b6e75678d04f00f558b0f47825612fc"
V060_TIMESTAMP = 0x6AAA07D1
V060_IMAGE_SIZE = 0x05228000

EXPECTED_PROLOGUE = bytes.fromhex(
    "55 41 57 41 56 41 55 41 54 56 57 53"
)

CAMERA_RVA = 0x009CEBF0
CANDIDATE_RVAS = (0x00AC2AE0, 0x00AC36F0, 0x00AC4300)
CORE_WRAPPER_RVA = 0x016D2740
CORE_RUNNER_ANCHOR_RVA = 0x016D3880

CAMERA_BODY_SIZE = 0x1AB5
CAMERA_EVIDENCE = {
    "zoom +0xE0": bytes.fromhex("F3 0F 10 86 E0 00 00 00"),
    "center pair +0xE4": bytes.fromhex("F2 0F 10 86 E4 00 00 00"),
    "mode +0x100": bytes.fromhex("44 0F B6 A6 00 01 00 00"),
    "pan vector +0x428": bytes.fromhex("48 8D 86 28 04 00 00"),
    "pan X write +0x428": bytes.fromhex("C7 86 28 04 00 00"),
    "pan Y write +0x42C": bytes.fromhex("C7 86 2C 04 00 00"),
}


@dataclass(frozen=True)
class Section:
    name: str
    rva: int
    virtual_size: int
    raw_offset: int
    raw_size: int

    @property
    def span(self) -> int:
        return max(self.virtual_size, self.raw_size)


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
            sections.append(
                Section(name, virtual_address, virtual_size, raw_offset, raw_size)
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

    def bytes_at(self, rva: int, size: int) -> bytes:
        offset = self.rva_to_offset(rva)
        return self.data[offset : offset + size]

    def contains_rel32_call(self, start_rva: int, size: int, target_rva: int) -> bool:
        block = self.bytes_at(start_rva, size)
        for index in range(len(block) - 4):
            if block[index] != 0xE8:
                continue
            displacement = struct.unpack_from("<i", block, index + 1)[0]
            resolved = start_rva + index + 5 + displacement
            if resolved == target_rva:
                return True
        return False


def fmt_bytes(data: bytes) -> str:
    return " ".join(f"{byte:02X}" for byte in data)


def safe_bytes(image: PeImage, rva: int, size: int) -> bytes | None:
    try:
        data = image.bytes_at(rva, size)
    except ValueError:
        return None
    return data if len(data) == size else None


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("exe", type=Path, help="path to current TeamfightManager2.exe")
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("tfm2-current-build-probe.txt"),
        help="report path (default: tfm2-current-build-probe.txt)",
    )
    args = parser.parse_args()

    image = PeImage(args.exe.resolve())
    digest = hashlib.sha256(image.data).hexdigest()

    lines: list[str] = [
        "TFM2 DIRECT CONTROL - CURRENT BUILD COMPATIBILITY TRIAGE",
        f"Executable: {image.path}",
        f"SHA-256: {digest}",
        f"PE timestamp: 0x{image.timestamp:08X}",
        f"Image size: 0x{image.image_size:08X}",
        f"Image base: 0x{image.image_base:X}",
        "",
        "Reference profile: physically validated Teamfight Manager 2 v0.6.0",
        f"  SHA-256: {V060_SHA256}",
        f"  PE timestamp: 0x{V060_TIMESTAMP:08X}",
        f"  Image size: 0x{V060_IMAGE_SIZE:08X}",
        "",
        "Read-only static analysis. No game files or process memory were modified.",
        "",
    ]

    same_rva_ok = True

    lines.append("===== SAME-RVA NATIVE TARGET CHECKS =====")

    for name, rva in (
        ("camera", CAMERA_RVA),
        ("Candidate A", CANDIDATE_RVAS[0]),
        ("Candidate B", CANDIDATE_RVAS[1]),
        ("Candidate C", CANDIDATE_RVAS[2]),
    ):
        actual = safe_bytes(image, rva, len(EXPECTED_PROLOGUE))
        ok = actual == EXPECTED_PROLOGUE
        same_rva_ok &= ok
        lines.append(
            f"{name:11s} RVA 0x{rva:08X}: "
            + ("PASS" if ok else "FAIL")
            + f" | {fmt_bytes(actual) if actual is not None else '<unmapped>'}"
        )

    lines.append("")
    lines.append("===== CANDIDATE -> COMMON WRAPPER CHECKS =====")
    for name, rva in zip(("Candidate A", "Candidate B", "Candidate C"), CANDIDATE_RVAS):
        try:
            ok = image.contains_rel32_call(rva, 0x300, CORE_WRAPPER_RVA)
        except ValueError:
            ok = False
        same_rva_ok &= ok
        lines.append(
            f"{name:11s} calls 0x{CORE_WRAPPER_RVA:08X}: "
            + ("PASS" if ok else "FAIL")
        )

    lines.append("")
    lines.append("===== CAMERA LAYOUT EVIDENCE AT v0.6.0 HANDLER =====")
    camera_body = safe_bytes(image, CAMERA_RVA, CAMERA_BODY_SIZE)
    if camera_body is None:
        same_rva_ok = False
        lines.append("<camera body unavailable>")
    else:
        for label, signature in CAMERA_EVIDENCE.items():
            ok = signature in camera_body
            same_rva_ok &= ok
            where = camera_body.find(signature)
            suffix = f" @ +0x{where:X}" if where >= 0 else ""
            lines.append(f"{label:24s}: {'PASS' if ok else 'FAIL'}{suffix}")

    lines.append("")
    lines.append("===== WRAPPER / RUNNER ANCHOR BYTES =====")
    for label, rva in (
        ("common wrapper", CORE_WRAPPER_RVA),
        ("runner anchor", CORE_RUNNER_ANCHOR_RVA),
    ):
        actual = safe_bytes(image, rva, 32)
        lines.append(
            f"{label:14s} RVA 0x{rva:08X}: "
            f"{fmt_bytes(actual) if actual is not None else '<unmapped>'}"
        )

    lines.append("")
    lines.append("===== TRIAGE VERDICT =====")
    if same_rva_ok:
        lines.extend(
            [
                "SAME-RVA SHAPE: PASS",
                "The current executable still matches every machine-level v0.6.0",
                "target/layout check performed here. This is strong evidence that the",
                "native layout may be unchanged and only the guarded build fingerprint",
                "needs a new profile. Runtime validation is still required before merge.",
            ]
        )
    else:
        lines.extend(
            [
                "SAME-RVA SHAPE: FAIL",
                "At least one physically validated v0.6.0 native assumption changed.",
                "Do NOT add the new PE fingerprint to the old profile blindly.",
                "Relocate/revalidate the failed simulation/camera target(s) first.",
            ]
        )

    output = args.output.resolve()
    output.write_text("\n".join(lines), encoding="utf-8")
    print(output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
