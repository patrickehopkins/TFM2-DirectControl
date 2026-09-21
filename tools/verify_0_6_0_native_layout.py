#!/usr/bin/env python3
"""Verify Direct Control's native v0.6.0 targets without external packages."""

from __future__ import annotations

import argparse
import hashlib
import struct
from pathlib import Path


EXPECTED_SHA256 = "aecb984a2c7ae187092a399b725d30db6b6e75678d04f00f558b0f47825612fc"
EXPECTED_TIMESTAMP = 0x6AAA07D1
EXPECTED_IMAGE_SIZE = 0x05228000
EXPECTED_PROLOGUE = bytes.fromhex("55 41 57 41 56 41 55 41 54 56 57 53")

CAMERA_RVA = 0x009CEBF0
CANDIDATE_RVAS = (0x00AC2AE0, 0x00AC36F0, 0x00AC4300)
CORE_WRAPPER_RVA = 0x016D2740


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
        self.image_size = struct.unpack_from("<I", self.data, optional_offset + 56)[0]

        self.sections: list[tuple[str, int, int, int, int]] = []
        section_offset = optional_offset + optional_size
        for index in range(section_count):
            offset = section_offset + index * 40
            name = self.data[offset : offset + 8].rstrip(b"\0").decode("ascii", "replace")
            virtual_size, virtual_address, raw_size, raw_offset = struct.unpack_from(
                "<IIII", self.data, offset + 8
            )
            self.sections.append(
                (name, virtual_address, virtual_size, raw_offset, raw_size)
            )

    def rva_to_offset(self, rva: int) -> int:
        for _, virtual_address, virtual_size, raw_offset, raw_size in self.sections:
            span = max(virtual_size, raw_size)
            if virtual_address <= rva < virtual_address + span:
                return raw_offset + rva - virtual_address
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
            if start_rva + index + 5 + displacement == target_rva:
                return True
        return False


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def verify(path: Path) -> None:
    image = PeImage(path)
    digest = hashlib.sha256(image.data).hexdigest()

    require(digest == EXPECTED_SHA256, f"unexpected SHA-256: {digest}")
    require(
        image.timestamp == EXPECTED_TIMESTAMP,
        f"unexpected PE timestamp: 0x{image.timestamp:08X}",
    )
    require(
        image.image_size == EXPECTED_IMAGE_SIZE,
        f"unexpected PE image size: 0x{image.image_size:08X}",
    )

    for name, rva in (
        ("camera", CAMERA_RVA),
        ("Candidate A", CANDIDATE_RVAS[0]),
        ("Candidate B", CANDIDATE_RVAS[1]),
        ("Candidate C", CANDIDATE_RVAS[2]),
    ):
        require(
            image.bytes_at(rva, len(EXPECTED_PROLOGUE)) == EXPECTED_PROLOGUE,
            f"{name} prologue mismatch at RVA 0x{rva:X}",
        )

    for name, rva in zip(("Candidate A", "Candidate B", "Candidate C"), CANDIDATE_RVAS):
        require(
            image.contains_rel32_call(rva, 0x300, CORE_WRAPPER_RVA),
            f"{name} no longer calls the wrapper at RVA 0x{CORE_WRAPPER_RVA:X}",
        )

    camera_body = image.bytes_at(CAMERA_RVA, 0x1AB5)
    camera_evidence = {
        "zoom +0xE0": bytes.fromhex("F3 0F 10 86 E0 00 00 00"),
        "center pair +0xE4": bytes.fromhex("F2 0F 10 86 E4 00 00 00"),
        "mode +0x100": bytes.fromhex("44 0F B6 A6 00 01 00 00"),
        "pan vector +0x428": bytes.fromhex("48 8D 86 28 04 00 00"),
        "pan X write +0x428": bytes.fromhex("C7 86 28 04 00 00"),
        "pan Y write +0x42C": bytes.fromhex("C7 86 2C 04 00 00"),
    }
    for label, signature in camera_evidence.items():
        require(signature in camera_body, f"missing camera-layout evidence for {label}")

    print(f"PASS: {path}")
    print(f"  SHA-256: {digest}")
    print(f"  PE: timestamp=0x{image.timestamp:08X}, image=0x{image.image_size:08X}")
    print("  Native camera and Candidate A/B/C targets match the v0.6.0 profile")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("exe", type=Path, help="path to the v0.6.0 TeamfightManager2.exe")
    args = parser.parse_args()
    try:
        verify(args.exe)
    except (OSError, ValueError) as error:
        parser.exit(1, f"FAIL: {error}\n")


if __name__ == "__main__":
    main()
