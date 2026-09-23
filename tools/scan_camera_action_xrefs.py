#!/usr/bin/env python3
"""Read-only spectator camera semantic-action discovery for Teamfight Manager 2.

TFM2's Rust binaries store many shortcut/action names as slices inside large
concatenated string blobs rather than as independent NUL-terminated C strings.
This probe therefore searches for substring starts, Rust-style &str descriptors
(pointer + usize length), direct/nearby RIP-relative references to those
descriptors, and the native match-view UI paths for the same vision controls.

The goal is to find binding-independent native routes for spectator camera actions:
    in_game_camera_all / team0 / team1
    in_game_auto_follow
    in_game_follow_own_{top,jungle,mid,bottom,support}
    in_game_follow_enemy_{top,jungle,mid,bottom,support}

The Direct Control Space-follow investigation specifically uses the role-follow actions to locate
the real native follow dispatcher/state instead of synthesizing keyboard input.

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
    ("follow", "in_game_follow_own_top"),
    ("follow", "in_game_follow_own_jungle"),
    ("follow", "in_game_follow_own_mid"),
    ("follow", "in_game_follow_own_bottom"),
    ("follow", "in_game_follow_own_support"),
    ("follow", "in_game_follow_enemy_top"),
    ("follow", "in_game_follow_enemy_jungle"),
    ("follow", "in_game_follow_enemy_mid"),
    ("follow", "in_game_follow_enemy_bottom"),
    ("follow", "in_game_follow_enemy_support"),
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
class RuntimeFunction:
    start: int
    end: int


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

    def runtime_functions(self) -> list[RuntimeFunction]:
        pdata = next((section for section in self.sections if section.name == ".pdata"), None)
        if pdata is None:
            return []

        out: list[RuntimeFunction] = []
        data = self.data[pdata.raw_offset : pdata.raw_offset + pdata.raw_size]
        for offset in range(0, len(data) - 11, 12):
            start, end, _unwind = struct.unpack_from("<III", data, offset)
            if start == 0 or end <= start:
                continue
            out.append(RuntimeFunction(start, end))
        out.sort(key=lambda fn: fn.start)
        return out


def owner_of(functions: list[RuntimeFunction], rva: int) -> RuntimeFunction | None:
    lo = 0
    hi = len(functions)
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


def collect_rel32_calls(image: PeImage) -> list[tuple[int, int]]:
    calls: list[tuple[int, int]] = []
    for section in image.sections:
        if not section.executable or section.raw_size < 5:
            continue
        data = image.data[section.raw_offset : section.raw_offset + section.raw_size]
        for i in range(len(data) - 4):
            if data[i] != 0xE8:
                continue
            disp = struct.unpack_from("<i", data, i + 1)[0]
            call_rva = section.rva + i
            target = call_rva + 5 + disp
            calls.append((call_rva, target))
    return calls


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


def decode_action_jump_table(
    image: PeImage,
    mapper: RuntimeFunction,
) -> tuple[int, list[tuple[int, int, str]]] | None:
    """Recover byte discriminant -> action name from the Rust enum name mapper."""

    try:
        code = image.bytes_at_rva(mapper.start, min(mapper.end - mapper.start, 0x80))
    except ValueError:
        return None

    signature = b"\x49\x63\x0c\x88\x4c\x01\xc1\xff\xe1"
    sig_at = code.find(signature)
    if sig_at < 7:
        return None

    lea_at = sig_at - 7
    if code[lea_at : lea_at + 3] != b"\x4c\x8d\x05":
        return None
    disp = struct.unpack_from("<i", code, lea_at + 3)[0]
    lea_rva = mapper.start + lea_at
    table_rva = lea_rva + 7 + disp

    decoded: list[tuple[int, int, str]] = []
    invalid_run = 0
    for discriminant in range(256):
        try:
            entry = struct.unpack("<i", image.bytes_at_rva(table_rva + discriminant * 4, 4))[0]
        except ValueError:
            break
        target = table_rva + entry
        if not (mapper.start <= target < mapper.end):
            invalid_run += 1
            if decoded and invalid_run >= 4:
                break
            continue

        invalid_run = 0
        try:
            case = image.bytes_at_rva(target, min(0x30, mapper.end - target))
        except ValueError:
            continue

        name = None
        for i in range(max(0, len(case) - 7)):
            if case[i : i + 3] != b"\x4c\x8d\x05":
                continue
            string_disp = struct.unpack_from("<i", case, i + 3)[0]
            string_rva = target + i + 7 + string_disp
            length = None
            for j in range(i + 7, min(len(case) - 5, i + 0x18)):
                if case[j : j + 2] == b"\x41\xb9":
                    length = struct.unpack_from("<I", case, j + 2)[0]
                    break
            if length is None or length == 0 or length > 128:
                continue
            try:
                raw = image.bytes_at_rva(string_rva, length)
            except ValueError:
                continue
            if any(byte < 0x20 or byte > 0x7E for byte in raw):
                continue
            try:
                name = raw.decode("ascii")
            except UnicodeDecodeError:
                continue
            break

        if name:
            decoded.append((discriminant, target, name))

    return table_rva, decoded


def index_small_enum_immediates(
    image: PeImage,
    wanted_values: set[int],
) -> dict[int, list[tuple[int, str]]]:
    """Index common x64 materializations for all wanted u8 enum values in one pass."""

    wanted = {value for value in wanted_values if 0 <= value <= 0xFF}
    out: dict[int, list[tuple[int, str]]] = {value: [] for value in wanted}
    if not wanted:
        return out

    for section in image.sections:
        if not section.executable:
            continue
        data = image.data[section.raw_offset : section.raw_offset + section.raw_size]
        base = section.rva
        n = len(data)

        # Single linear pass. Recognize:
        #   B8+r imm32
        #   41 B8+r imm32
        #   C6 44 24 disp8 imm8
        #   C6 45 disp8 imm8
        #   41 C6 44 24 disp8 imm8
        i = 0
        while i < n:
            b0 = data[i]

            if 0xB8 <= b0 <= 0xBF and i + 5 <= n:
                value32 = struct.unpack_from("<I", data, i + 1)[0]
                if value32 in wanted:
                    out[value32].append((base + i, "mov32"))
                i += 5
                continue

            if (
                b0 == 0x41
                and i + 6 <= n
                and 0xB8 <= data[i + 1] <= 0xBF
            ):
                value32 = struct.unpack_from("<I", data, i + 2)[0]
                if value32 in wanted:
                    out[value32].append((base + i, "mov32-rex"))
                i += 6
                continue

            if (
                b0 == 0xC6
                and i + 5 <= n
                and data[i + 1] == 0x44
                and data[i + 2] == 0x24
            ):
                value8 = data[i + 4]
                if value8 in wanted:
                    out[value8].append((base + i, "store-stack8"))
                i += 5
                continue

            if b0 == 0xC6 and i + 4 <= n and data[i + 1] == 0x45:
                value8 = data[i + 3]
                if value8 in wanted:
                    out[value8].append((base + i, "store-local8"))
                i += 4
                continue

            if (
                b0 == 0x41
                and i + 6 <= n
                and data[i + 1] == 0xC6
                and data[i + 2] == 0x44
                and data[i + 3] == 0x24
            ):
                value8 = data[i + 5]
                if value8 in wanted:
                    out[value8].append((base + i, "store-r12-8"))
                i += 6
                continue

            i += 1

    for value in out:
        out[value] = sorted(set(out[value]))
    return out



def first_rel32_call_after(
    image: PeImage,
    site_rva: int,
    max_distance: int = 0x28,
) -> tuple[int, int] | None:
    """Return the first direct call shortly after an action-id materialization."""

    try:
        block = image.bytes_at_rva(site_rva, max_distance)
    except ValueError:
        return None
    for i in range(len(block) - 4):
        if block[i] != 0xE8:
            continue
        disp = struct.unpack_from("<i", block, i + 1)[0]
        call_rva = site_rva + i
        return call_rva, call_rva + 5 + disp
    return None


def printable_at(image: PeImage, rva: int, limit: int = 72) -> str | None:
    """Return a short printable run when an RVA appears to point at text."""

    try:
        raw = image.bytes_at_rva(rva, limit)
    except ValueError:
        return None
    out = bytearray()
    for byte in raw:
        if byte == 0:
            break
        if byte < 0x20 or byte > 0x7E:
            break
        out.append(byte)
    if len(out) < 4:
        return None
    try:
        return out.decode("ascii")
    except UnicodeDecodeError:
        return None


def nearby_rip_text(
    image: PeImage,
    all_refs: list[tuple[int, int, str]],
    site_rva: int,
    before: int = 0x28,
    after: int = 0x10,
) -> list[tuple[int, int, str, str]]:
    out: list[tuple[int, int, str, str]] = []
    low = site_rva - before
    high = site_rva + after

    # collect_rip_refs walks executable sections/address order, so binary-search the xref RVA
    # instead of rescanning ~470k references for every action site.
    lo = 0
    hi = len(all_refs)
    while lo < hi:
        mid = (lo + hi) // 2
        if all_refs[mid][0] < low:
            lo = mid + 1
        else:
            hi = mid

    for xref, resolved, kind in all_refs[lo:]:
        if xref > high:
            break
        text = printable_at(image, resolved)
        if text:
            out.append((xref, resolved, kind, text))
    return out



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
    runtime_functions = image.runtime_functions()
    all_calls = collect_rel32_calls(image)

    lines: list[str] = [
        "TFM2 DIRECT CONTROL - SPECTATOR CAMERA SEMANTIC ACTION REPORT",
        f"Executable: {image.path}",
        f"SHA-256: {sha256(image.data)}",
        f"PE timestamp: 0x{image.timestamp:08X}",
        f"Image size: 0x{image.image_size:08X}",
        f"Image base: 0x{image.image_base:X}",
        f"Collected RIP-relative LEA/MOV refs: {len(all_refs)}",
        f"Runtime functions from .pdata: {len(runtime_functions)}",
        f"Collected rel32 calls: {len(all_calls)}",
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
    follow_names = [
        "in_game_follow_own_top",
        "in_game_follow_own_jungle",
        "in_game_follow_own_mid",
        "in_game_follow_own_bottom",
        "in_game_follow_own_support",
        "in_game_follow_enemy_top",
        "in_game_follow_enemy_jungle",
        "in_game_follow_enemy_mid",
        "in_game_follow_enemy_bottom",
        "in_game_follow_enemy_support",
    ]
    action_refs = sorted(
        set(
            direct_code_refs.get("in_game_camera_all", [])
            + direct_code_refs.get("in_game_camera_team0", [])
            + direct_code_refs.get("in_game_camera_team1", [])
            + direct_code_refs.get("in_game_auto_follow", [])
            + [
                ref
                for name in follow_names
                for ref in direct_code_refs.get(name, [])
            ]
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

    follow_refs = sorted(
        set(
            ref
            for name in follow_names
            for ref in direct_code_refs.get(name, [])
        )
    )
    if follow_refs:
        # Follow role-name conversion/dispatch tends to be a tight switch cluster. A wider window
        # helps recover the discriminant and nearby call targets in one report.
        emit_code_window(
            lines,
            image,
            "ROLE FOLLOW ACTION CODE WINDOW",
            max(0, min(follow_refs) - 0x300),
            max(follow_refs) + 0x300,
        )

    # Recover enum discriminants from the byte-indexed Rust jump table. The action-name strings
    # are serialization/debug metadata; discriminants let us search real camera/input code.
    if follow_refs:
        mapper_owner_for_ids = owner_of(runtime_functions, min(follow_refs))
        if mapper_owner_for_ids:
            decoded = decode_action_jump_table(image, mapper_owner_for_ids)
            if decoded:
                jump_table_rva, action_ids = decoded
                lines.append("===== ACTION ENUM DISCRIMINANTS =====")
                lines.append(
                    f"mapper 0x{mapper_owner_for_ids.start:08X}..0x{mapper_owner_for_ids.end:08X}; "
                    f"jump table=0x{jump_table_rva:08X}; decoded={len(action_ids)}"
                )
                for discriminant, target, name in action_ids:
                    if name.startswith("in_game_"):
                        lines.append(
                            f"id={discriminant:3d} (0x{discriminant:02X}) "
                            f"case=0x{target:08X} name={name}"
                        )
                lines.append("")

                id_by_name = {name: discriminant for discriminant, _target, name in action_ids}
                follow_ids = {
                    name: id_by_name[name]
                    for name in follow_names
                    if name in id_by_name
                }
                lines.append("===== FOLLOW ENUM IMMEDIATE CLUSTERS =====")
                if len(follow_ids) < len(follow_names):
                    missing = [name for name in follow_names if name not in follow_ids]
                    lines.append("missing decoded follow ids: " + ", ".join(missing))
                else:
                    follow_site_index = index_small_enum_immediates(
                        image, set(follow_ids.values())
                    )
                    sites_by_name = {
                        name: follow_site_index.get(value, [])
                        for name, value in follow_ids.items()
                    }
                    owners: dict[tuple[int, int], dict[str, list[tuple[int, str]]]] = {}
                    for name, sites in sites_by_name.items():
                        for site, kind in sites:
                            owner = owner_of(runtime_functions, site)
                            if owner is None:
                                continue
                            owners.setdefault((owner.start, owner.end), {}).setdefault(name, []).append(
                                (site, kind)
                            )

                    ranked = sorted(
                        (
                            (len(by_name), sum(len(v) for v in by_name.values()), start, end, by_name)
                            for (start, end), by_name in owners.items()
                            if len(by_name) >= 3
                        ),
                        reverse=True,
                    )
                    if not ranked:
                        lines.append("<no function contains patterned immediates for >=3 follow ids>")
                    for distinct, site_count, start, end, by_name in ranked[:20]:
                        lines.append(
                            f"owner 0x{start:08X}..0x{end:08X} size=0x{end-start:X} "
                            f"distinct_follow_ids={distinct} sites={site_count}"
                        )
                        for name in follow_names:
                            hits = by_name.get(name)
                            if hits:
                                value = follow_ids[name]
                                lines.append(
                                    f"  {name} id=0x{value:02X}: "
                                    + ", ".join(f"0x{site:08X}/{kind}" for site, kind in hits[:12])
                                )
                        if end - start <= 0x1800:
                            emit_code_window(
                                lines,
                                image,
                                f"FOLLOW-ID OWNER 0x{start:08X}",
                                start,
                                end,
                            )
                        else:
                            flat_hits = sorted(
                                site for hits in by_name.values() for site, _kind in hits
                            )
                            for site in flat_hits[:4]:
                                emit_code_window(
                                    lines,
                                    image,
                                    f"FOLLOW-ID WINDOW 0x{site:08X}",
                                    max(start, site - 0x140),
                                    min(end, site + 0x1C0),
                                )
                lines.append("")

    # Group real code sites by the direct callee immediately following an action-id load. A native
    # action-state query should recur for several follow ids at the same callee, whereas generated
    # enum serialization tends to fan out through unrelated per-variant code.
    if follow_refs and 'action_ids' in locals():
        ingame_ids = {
            name: discriminant
            for discriminant, _target, name in action_ids
            if name.startswith("in_game_")
        }
        follow_name_set = set(follow_names)
        call_groups: dict[
            tuple[int, int, int],
            dict[str, list[tuple[int, str, int]]],
        ] = {}

        ingame_site_index = index_small_enum_immediates(
            image, set(ingame_ids.values())
        )
        for name, value in ingame_ids.items():
            for site, kind in ingame_site_index.get(value, []):
                owner = owner_of(runtime_functions, site)
                if owner is None:
                    continue
                call = first_rel32_call_after(image, site)
                if call is None:
                    continue
                call_rva, callee = call
                key = (owner.start, owner.end, callee)
                call_groups.setdefault(key, {}).setdefault(name, []).append(
                    (site, kind, call_rva)
                )

        ranked_call_groups = sorted(
            (
                (
                    len(follow_name_set.intersection(by_name)),
                    len(by_name),
                    sum(len(v) for v in by_name.values()),
                    owner_start,
                    owner_end,
                    callee,
                    by_name,
                )
                for (owner_start, owner_end, callee), by_name in call_groups.items()
                if len(follow_name_set.intersection(by_name)) >= 3
            ),
            reverse=True,
        )

        lines.append("===== ACTION-ID NEAR-CALL GROUPS =====")
        if not ranked_call_groups:
            lines.append("<no direct callee is paired with >=3 distinct follow ids>")
        for (
            follow_count,
            all_count,
            occurrence_count,
            owner_start,
            owner_end,
            callee,
            by_name,
        ) in ranked_call_groups[:24]:
            callee_owner = owner_of(runtime_functions, callee)
            callee_text = (
                f"0x{callee_owner.start:08X}..0x{callee_owner.end:08X}"
                if callee_owner
                else "<no .pdata owner>"
            )
            lines.append(
                f"owner 0x{owner_start:08X}..0x{owner_end:08X} "
                f"callee=0x{callee:08X} callee_owner={callee_text} "
                f"follow_ids={follow_count} all_ingame_ids={all_count} occurrences={occurrence_count}"
            )
            ordered_names = sorted(
                by_name,
                key=lambda item: (ingame_ids.get(item, 0xFFFF), item),
            )
            for name in ordered_names:
                hits = by_name[name]
                hit_text = ", ".join(
                    f"0x{site:08X}/{kind}->call@0x{call_rva:08X}"
                    for site, kind, call_rva in hits[:8]
                )
                lines.append(
                    f"  id=0x{ingame_ids[name]:02X} {name}: {hit_text}"
                )
                for site, _kind, _call_rva in hits[:1]:
                    texts = nearby_rip_text(image, all_refs, site)
                    for xref, resolved, ref_kind, text_value in texts[:4]:
                        lines.append(
                            f"    nearby {ref_kind} 0x{xref:08X}->0x{resolved:08X}: {text_value!r}"
                        )
            lines.append("")

        # Dump the callees behind the strongest groups once. If one is the native action-state
        # predicate, this exposes the exact ABI/prologue needed for a version-checked detour.
        dumped_callees: set[int] = set()
        for (
            _follow_count,
            _all_count,
            _occurrence_count,
            _owner_start,
            _owner_end,
            callee,
            _by_name,
        ) in ranked_call_groups[:12]:
            if callee in dumped_callees:
                continue
            dumped_callees.add(callee)
            callee_owner = owner_of(runtime_functions, callee)
            if callee_owner is None:
                continue
            span = callee_owner.end - callee_owner.start
            emit_code_window(
                lines,
                image,
                f"ACTION-ID CALLEE 0x{callee_owner.start:08X}",
                callee_owner.start,
                min(callee_owner.end, callee_owner.start + min(span, 0x700)),
            )

    # Deeper pass: all semantic action descriptors form one contiguous 16-byte table. References
    # into any address inside that table are more useful than exact references to individual members
    # because Rust code commonly addresses the table base and indexes into it.
    action_descriptor_rvas = sorted(
        {
            descriptor
            for name, descriptors in descriptor_centers.items()
            if name.startswith("in_game_") and not name.startswith("speed_buttons.")
            for descriptor in descriptors
            if decode_rust_str_descriptor(image, descriptor) is not None
        }
    )
    if action_descriptor_rvas:
        table_start = min(action_descriptor_rvas)
        table_end = max(action_descriptor_rvas) + 0x10
        table_refs = sorted(
            ref for ref in all_refs if table_start <= ref[1] < table_end
        )

        lines.append("===== ACTION DESCRIPTOR TABLE EXECUTABLE REFS =====")
        lines.append(
            f"table 0x{table_start:08X}..0x{table_end:08X}; refs={len(table_refs)}"
        )
        grouped: dict[tuple[int, int], list[tuple[int, int, str]]] = {}
        for ref in table_refs:
            owner = owner_of(runtime_functions, ref[0])
            key = (owner.start, owner.end) if owner else (ref[0], ref[0] + 1)
            grouped.setdefault(key, []).append(ref)

        for (owner_start, owner_end), refs in sorted(grouped.items()):
            lines.append(
                f"owner 0x{owner_start:08X}..0x{owner_end:08X} "
                f"size=0x{owner_end-owner_start:X} refs={len(refs)}"
            )
            for xref_rva, resolved, kind in refs[:32]:
                lines.append(
                    f"  0x{xref_rva:08X} {kind} -> 0x{resolved:08X} "
                    f"(table+0x{resolved-table_start:X})"
                )
            if owner_end - owner_start <= 0x1800:
                emit_code_window(
                    lines,
                    image,
                    f"ACTION TABLE OWNER 0x{owner_start:08X}",
                    owner_start,
                    owner_end,
                )
            else:
                emit_code_window(
                    lines,
                    image,
                    f"ACTION TABLE OWNER HEAD 0x{owner_start:08X}",
                    owner_start,
                    min(owner_end, owner_start + 0x700),
                )

    # The dense 0x0215Dxxx cluster is an enum->name mapper. Find its real .pdata owner and every
    # direct caller. Callers are candidates for semantic shortcut registration/query code.
    if follow_refs:
        mapper_owner = owner_of(runtime_functions, min(follow_refs))
        if mapper_owner:
            mapper_callers = sorted(
                call_rva for call_rva, target in all_calls if target == mapper_owner.start
            )
            lines.append("===== ACTION NAME MAPPER CALLERS =====")
            lines.append(
                f"mapper owner 0x{mapper_owner.start:08X}..0x{mapper_owner.end:08X} "
                f"size=0x{mapper_owner.end-mapper_owner.start:X}; callers={len(mapper_callers)}"
            )
            emit_code_window(
                lines,
                image,
                "ACTION NAME MAPPER FUNCTION",
                mapper_owner.start,
                mapper_owner.end,
            )

            caller_owners: dict[tuple[int, int], list[int]] = {}
            for call_rva in mapper_callers:
                owner = owner_of(runtime_functions, call_rva)
                key = (owner.start, owner.end) if owner else (call_rva, call_rva + 1)
                caller_owners.setdefault(key, []).append(call_rva)

            for (owner_start, owner_end), calls in sorted(caller_owners.items()):
                lines.append(
                    f"caller owner 0x{owner_start:08X}..0x{owner_end:08X} "
                    f"size=0x{owner_end-owner_start:X}; callsites="
                    + ", ".join(f"0x{call:08X}" for call in calls)
                )
                if owner_end - owner_start <= 0x1400:
                    emit_code_window(
                        lines,
                        image,
                        f"MAPPER CALLER OWNER 0x{owner_start:08X}",
                        owner_start,
                        owner_end,
                    )
                else:
                    for call_rva in calls[:6]:
                        emit_code_window(
                            lines,
                            image,
                            f"MAPPER CALLER WINDOW 0x{call_rva:08X}",
                            max(owner_start, call_rva - 0x180),
                            min(owner_end, call_rva + 0x220),
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
