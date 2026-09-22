#!/usr/bin/env python3
"""Read-only spectator camera semantic-action discovery for Teamfight Manager 2.

TFM2's Rust binaries store many shortcut/action names as slices inside large
concatenated string blobs rather than as independent NUL-terminated C strings.
This probe therefore searches for substring starts, Rust-style &str descriptors
(pointer + usize length), direct/nearby RIP-relative references to those
descriptors, and the native match-view UI paths for the same vision controls.

The goal is to find a binding-independent native route for:
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


ANCHORS = (
    ("action", "in_game_camera_all"),
    ("action", "in_game_camera_team0"),
    ("action", "in_game_camera_team1"),
    ("action", "in_game_auto_follow"),
    ("ui", "speed_buttons.view_all"),
    ("ui", "speed_buttons.view_blue"),
    ("ui", "speed_buttons.view_red"),
    ("ui", "center_data.camera_buttons."),
    ("ui", "wide_data.camera_buttons."),
)

CONTEXT_BEFORE = 0x50
CONTEXT_AFTER = 0x90
STRING_CONTEXT = 0x70
CALL_SCAN_BEFORE = 0x80
CALL_SCAN_AFTER = 0x100
NEAR_DATA_RADIUS = 0x100


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
        self.sections: list[Section] = []
        for index in range(section_count):
            offset = section_offset + index * 40
            name = self.data[offset : offset + 8].rstrip(b"\0").decode("ascii", "replace")
            virtual_size, virtual_address, raw_size, raw_offset = struct.unpack_from(
                "<IIII", self.data, offset + 8
            )
            characteristics = struct.unpack_from("<I", self.data, offset + 36)[0]
            self.sections.append(
                Section(
                    name=name,
                    rva=virtual_address,
                    virtual_size=virtual_size,
                    raw_offset=raw_offset,
                    raw_size=raw_size,
                    characteristics=characteristics,
                )
            )

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


def ascii_context(image: PeImage, rva: int) -> str:
    try:
        offset = image.rva_to_offset(rva)
    except ValueError:
        return "<unavailable>"
    start = max(0, offset - STRING_CONTEXT)
    end = min(len(image.data), offset + STRING_CONTEXT)
    chunk = image.data[start:end]
    text = "".join(chr(b) if 32 <= b <= 126 else "." for b in chunk)
    try:
        start_rva = image.offset_to_rva(start)
    except ValueError:
        start_rva = 0
    return f"    around RVA 0x{start_rva:08X}: {text}"


def find_substring_rvas(image: PeImage, text: str) -> list[int]:
    """Find raw ASCII substring starts; do not require a trailing NUL."""

    needle = text.encode("ascii")
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
    return sorted(set(out))


def find_pointer_occurrences(image: PeImage, target_rva: int) -> list[int]:
    """Find data locations containing the absolute VA of target_rva."""

    needle = struct.pack("<Q", image.image_base + target_rva)
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
    return sorted(set(out))


def descriptor_kind(image: PeImage, descriptor_rva: int, text_len: int) -> str:
    """Classify common pointer+length layouts at a pointer occurrence."""

    try:
        offset = image.rva_to_offset(descriptor_rva)
    except ValueError:
        return "pointer"

    if offset + 16 <= len(image.data):
        length64 = struct.unpack_from("<Q", image.data, offset + 8)[0]
        if length64 == text_len:
            return "Rust &str {ptr, usize_len}"

    if offset + 12 <= len(image.data):
        length32 = struct.unpack_from("<I", image.data, offset + 8)[0]
        if length32 == text_len:
            return "ptr + u32_len"

    return "pointer"


def collect_rip_refs(image: PeImage) -> list[tuple[int, int, str]]:
    """Collect common x64 RIP-relative LEA/MOV references once for the whole image."""

    refs: list[tuple[int, int, str]] = []
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
            refs.append((insn_rva, resolved, "lea" if opcode == 0x8D else "mov"))

    return refs


def refs_to(
    all_refs: list[tuple[int, int, str]], target_rva: int
) -> list[tuple[int, int, str]]:
    return [ref for ref in all_refs if ref[1] == target_rva]


def refs_near(
    all_refs: list[tuple[int, int, str]], target_rva: int, radius: int = NEAR_DATA_RADIUS
) -> list[tuple[int, int, str]]:
    return [
        ref
        for ref in all_refs
        if abs(ref[1] - target_rva) <= radius
    ]


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

    return sorted(set(out))


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


def emit_code_ref(
    lines: list[str],
    image: PeImage,
    xref_rva: int,
    resolved_rva: int,
    kind: str,
    label: str,
) -> None:
    lines.append(
        f"{label}: 0x{xref_rva:08X} ({kind}) -> data RVA 0x{resolved_rva:08X}"
    )
    calls = nearby_rel32_calls(image, xref_rva)
    if calls:
        lines.append("Nearby rel32 calls:")
        for call_rva, target in calls:
            marker = "  <== near xref" if abs(call_rva - xref_rva) <= 0x20 else ""
            lines.append(f"  CALL 0x{call_rva:08X} -> 0x{target:08X}{marker}")
    else:
        lines.append("Nearby rel32 calls: <none>")
    lines.append("Byte context:")
    lines.append(dump_xref_context(image, xref_rva))



def decode_rust_str_descriptor(image: PeImage, descriptor_rva: int) -> tuple[str, int, int] | None:
    """Decode a likely {absolute_ptr, usize_len} Rust &str descriptor."""

    try:
        offset = image.rva_to_offset(descriptor_rva)
    except ValueError:
        return None
    if offset + 16 > len(image.data):
        return None

    ptr_va, length = struct.unpack_from("<QQ", image.data, offset)
    if length == 0 or length > 128 or ptr_va < image.image_base:
        return None

    string_rva = ptr_va - image.image_base
    try:
        raw = image.bytes_at_rva(string_rva, length)
    except ValueError:
        return None
    if not raw or any(byte < 0x20 or byte > 0x7E for byte in raw):
        return None
    try:
        text = raw.decode("ascii")
    except UnicodeDecodeError:
        return None
    return text, string_rva, length


def emit_descriptor_table_near(
    lines: list[str],
    image: PeImage,
    all_refs: list[tuple[int, int, str]],
    center_rva: int,
    radius: int = 0x280,
) -> None:
    """Dump nearby 16-byte Rust string descriptors plus executable refs to each."""

    start = max(0, center_rva - radius)
    end = center_rva + radius
    # Align to 16 bytes because this table is a contiguous sequence of {ptr,len}.
    start &= ~0xF

    lines.append("===== LOCAL RUST STRING-DESCRIPTOR TABLE =====")
    lines.append(
        f"Scanning aligned descriptors around 0x{center_rva:08X} "
        f"(0x{start:08X}..0x{end:08X})"
    )

    found = 0
    for descriptor_rva in range(start, end + 1, 0x10):
        decoded = decode_rust_str_descriptor(image, descriptor_rva)
        if decoded is None:
            continue
        text, string_rva, length = decoded
        found += 1
        refs = refs_to(all_refs, descriptor_rva)
        ref_text = ", ".join(
            f"0x{xref:08X}/{kind}" for xref, _, kind in refs[:12]
        )
        lines.append(
            f"0x{descriptor_rva:08X}: ptr=0x{string_rva:08X} "
            f"len={length:>2} text={text!r}"
            + (f" | refs: {ref_text}" if ref_text else "")
        )

    if found == 0:
        lines.append("<no printable Rust &str descriptors decoded>")
    lines.append("")


def emit_code_window(
    lines: list[str],
    image: PeImage,
    title: str,
    start_rva: int,
    end_rva: int,
) -> None:
    lines.append(f"===== {title} =====")
    try:
        block = image.bytes_at_rva(start_rva, end_rva - start_rva)
    except ValueError as exc:
        lines.append(f"<unavailable: {exc}>")
        lines.append("")
        return
    lines.append(hexdump(block, start_rva))
    lines.append("")

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
    all_refs = collect_rip_refs(image)

    lines: list[str] = [
        "TFM2 DIRECT CONTROL - SPECTATOR CAMERA SEMANTIC ACTION REPORT",
        f"Executable: {image.path}",
        f"SHA-256: {sha256(image.data)}",
        f"PE timestamp: 0x{image.timestamp:08X}",
        f"Image size: 0x{image.image_size:08X}",
        f"Image base: 0x{image.image_base:X}",
        f"Collected RIP-relative LEA/MOV refs: {len(all_refs)}",
        "",
        "Read-only static analysis. No game files or process memory were modified.",
        "",
    ]

    discovered_code_refs: list[tuple[int, str]] = []
    descriptor_centers: dict[str, list[int]] = {}
    direct_code_refs: dict[str, list[int]] = {}

    for category, name in ANCHORS:
        lines.append(f"===== [{category}] {name} =====")
        string_rvas = find_substring_rvas(image, name)
        if not string_rvas:
            lines.append("ASCII substring: NOT FOUND")
            lines.append("")
            continue

        for string_rva in string_rvas:
            lines.append(f"Substring RVA: 0x{string_rva:08X}")
            lines.append(ascii_context(image, string_rva))

            direct = refs_to(all_refs, string_rva)
            if direct:
                direct_code_refs.setdefault(name, []).extend(
                    xref_rva for xref_rva, _, _ in direct
                )
                for xref_rva, resolved, kind in direct:
                    discovered_code_refs.append((xref_rva, name))
                    emit_code_ref(
                        lines, image, xref_rva, resolved, kind, "DIRECT STRING XREF"
                    )
            else:
                lines.append("Direct executable refs to substring: <none>")

            pointer_rvas = find_pointer_occurrences(image, string_rva)
            if pointer_rvas:
                descriptor_centers.setdefault(name, []).extend(pointer_rvas)
            if not pointer_rvas:
                lines.append("Absolute pointer/descriptor occurrences: <none>")
            else:
                lines.append("Absolute pointer/descriptor occurrences:")
                for descriptor_rva in pointer_rvas:
                    kind = descriptor_kind(image, descriptor_rva, len(name))
                    lines.append(
                        f"  data RVA 0x{descriptor_rva:08X}: {kind}"
                    )

                    exact_descriptor_refs = refs_to(all_refs, descriptor_rva)
                    for xref_rva, resolved, ref_kind in exact_descriptor_refs:
                        discovered_code_refs.append((xref_rva, name))
                        emit_code_ref(
                            lines,
                            image,
                            xref_rva,
                            resolved,
                            ref_kind,
                            "DESCRIPTOR XREF",
                        )

                    if not exact_descriptor_refs:
                        near = refs_near(all_refs, descriptor_rva)
                        # Near references are useful when code addresses the base of a
                        # static table rather than this exact {ptr,len} member.
                        near = sorted(
                            near,
                            key=lambda item: (abs(item[1] - descriptor_rva), item[0]),
                        )[:12]
                        if near:
                            lines.append(
                                f"  Nearby executable data refs (within 0x{NEAR_DATA_RADIUS:X}):"
                            )
                            for xref_rva, resolved, ref_kind in near:
                                delta = resolved - descriptor_rva
                                discovered_code_refs.append((xref_rva, name))
                                lines.append(
                                    f"    0x{xref_rva:08X} ({ref_kind}) -> "
                                    f"0x{resolved:08X} (descriptor {delta:+#x})"
                                )
                        else:
                            lines.append("  Executable refs to/near descriptor: <none>")

            lines.append("")

        lines.append("")

    # The action descriptors for all/team0/team1/auto-follow are contiguous in
    # the current Rust data table. Decode the local table and show exact code refs
    # so we can identify the input-query surface instead of treating nearby data
    # references as evidence.
    team0_descriptors = descriptor_centers.get("in_game_camera_team0", [])
    if team0_descriptors:
        emit_descriptor_table_near(
            lines, image, all_refs, min(team0_descriptors)
        )

    # The view_red/view_blue/view_all paths form a tight per-frame UI-selection
    # cluster. Dump the whole region so the state byte used to light those buttons
    # can be decoded in context.
    ui_refs = sorted(
        set(
            direct_code_refs.get("speed_buttons.view_red", [])
            + direct_code_refs.get("speed_buttons.view_blue", [])
            + direct_code_refs.get("speed_buttons.view_all", [])
        )
    )
    if ui_refs:
        low_cluster = [r for r in ui_refs if r < 0x0220_0000]
        if low_cluster:
            emit_code_window(
                lines,
                image,
                "FOG BUTTON STATE CODE WINDOW",
                max(0, min(low_cluster) - 0x120),
                max(low_cluster) + 0x180,
            )

    # The higher action-string cluster appears to convert internal action ids to
    # names. Dump its surrounding switch so the team0/team1 discriminants can be
    # recovered if useful.
    action_refs = sorted(
        set(
            direct_code_refs.get("in_game_camera_all", [])
            + direct_code_refs.get("in_game_camera_team0", [])
            + direct_code_refs.get("in_game_camera_team1", [])
            + direct_code_refs.get("in_game_auto_follow", [])
        )
    )
    high_action_refs = [r for r in action_refs if r >= 0x0260_0000]
    if high_action_refs:
        emit_code_window(
            lines,
            image,
            "CAMERA ACTION ID/NAME CODE WINDOW",
            max(0, min(high_action_refs) - 0x180),
            max(high_action_refs) + 0x180,
        )

    lines.append("===== CODE-XREF CLUSTERS =====")
    flattened = sorted(set(discovered_code_refs))
    if not flattened:
        lines.append("<no usable executable refs found>")
    else:
        for xref, name in flattened:
            neighbors = [
                (other_xref, other_name)
                for other_xref, other_name in flattened
                if other_name != name and abs(other_xref - xref) <= 0x600
            ]
            neighbor_text = ", ".join(
                f"{other_name}@0x{other_xref:08X}"
                for other_xref, other_name in neighbors[:12]
            )
            lines.append(
                f"{name}@0x{xref:08X}: "
                f"{neighbor_text if neighbor_text else '<no other anchor xref within 0x600>'}"
            )

    output = args.output.resolve()
    output.write_text("\n".join(lines), encoding="utf-8")
    print(output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
