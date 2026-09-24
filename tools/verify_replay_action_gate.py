#!/usr/bin/env python3
"""Read-only v0.6.1 replay action gate binary verification.

Checks exact executable identity, hook trampoline boundaries, and *runtime*
lookup calls followed by comparisons with native incoming keys. Run before
rebasing the native gate to another version. This is NOT a gameplay test.
"""
import argparse
import hashlib
import os
import struct
from pathlib import Path

SHA256 = "91084e9a29c70993595a1d7d0c22064bae0ee82b0fc773c696077d15c2268f98"
TIMESTAMP, IMAGE_SIZE = 0x6AB1D950, 0x05264000
LOOKUP_RVA = 0x021C4CE0
LOOKUP_PREFIX = bytes.fromhex("56 53 48 83 EC 28 89 D3 88 54 24 27")
CAMERA_RVA = 0x00C2DBE0
CAMERA_PREFIX = bytes.fromhex("55 41 57 41 56 41 55 41 54 56 57 53")
ACTION_LOOKUPS = {
    0x1B: (0x00C2EC07, 0x00C2ED1E),  # Highlight mode
    0x30: (0x00C2F4BD,),              # Previous highlight
    0x31: (0x00C2F51F,),              # Back 10 seconds
    0x32: (0x00C2F581,),              # Timeline pause
    0x33: (0x00C2F5C6,),              # Forward 10 seconds
    0x34: (0x00C2F628,),              # Next highlight
}
# Follow-up calls from the same native match handler.
ACTIONS_TO_NATIVE_OPS = {
    0x30: (0x00C2F4F6, 0x00C30E90),
    0x31: (0x00C2F558, 0x00C32290),
    0x32: (0x00C2F59D, 0x01D729F0),
    0x33: (0x00C2F5FF, 0x00C31D60),
    0x34: (0x00C2F661, 0x00C308D0),
}

class PE:
    def __init__(self, data):
        self.data = data
        p = struct.unpack_from("<I", data, 0x3C)[0]
        assert data[:2] == b"MZ" and data[p:p+4] == b"PE\\x00\\x00", "Not a PE image"
        self.timestamp = struct.unpack_from("<I", data, p+8)[0]
        h = p+24
        self.image_size = struct.unpack_from("<I", data, h+56)[0]
        count = struct.unpack_from("<H", data, p+6)[0]
        start = h+struct.unpack_from("<H", data, p+20)[0]
        self.sections = [
            struct.unpack_from("<IIII", data, start+40*i+8) for i in range(count)
        ]

    def read(self, rva, length):
        for _, s_rva, sz, offset in self.sections:
            if s_rva <= rva and rva + length <= s_rva + sz:
                p = offset + rva - s_rva
                return self.data[p:p+length]
        raise ValueError(f"Unmapped RVA 0x{rva:08X}")

    def call_target(self, rva):
        b = self.read(rva, 5)
        assert b[0] == 0xE8, f"Missing direct call at 0x{rva:08X}"
        return rva + 5 + struct.unpack("<i", b[1:])[0]


def verify(exe):
    data = exe.read_bytes()
    pe = PE(data)
    assert hashlib.sha256(data).hexdigest() == SHA256, "Wrong v0.6.1 executable/hash"
    assert (pe.timestamp, pe.image_size) == (TIMESTAMP, IMAGE_SIZE), "PE build changed"
    assert pe.read(LOOKUP_RVA, 12) == LOOKUP_PREFIX, "Binding getter prologue changed"
    assert pe.read(CAMERA_RVA, 12) == CAMERA_PREFIX, "Camera handler prologue changed"
    for action, sites in ACTION_LOOKUPS.items():
        for site in sites:
            assert pe.read(site-2, 2) == bytes((0xB2, action)), "Action argument changed"
            assert pe.call_target(site) == LOOKUP_RVA, "Runtime lookup target changed"
            following = pe.read(site+5, 28)
            assert b"\\x41\\x38\\xC6" in following or b"\\x41\\x38\\xC7" in following, (
                f"Incoming-key comparison absent near 0x{site:X}"
            )
    for action, (site, target) in ACTIONS_TO_NATIVE_OPS.items():
        assert pe.call_target(site) == target, f"Action 0x{action:X} native op changed"
    assert 0xFF not in pe.read(0x03BF8E05, 55), "0xFF no longer an unused default key"
    print("PASS: v0.6.1 identity, full instruction prologues, semantic action lookups")
    print("PASS: native time/highlight paths and reserved key sentinel")
    print("Physical remapped-shortcut, pause, camera, and Ctrl+End testing still required.")

if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    default = (Path(os.environ.get("ProgramFiles(x86)", "C:/Program Files (x86)"))
               / "Steam/steamapps/common/Teamfight Manager2/TeamfightManager2.exe")
    p.add_argument("--exe", type=Path, default=default)
    a = p.parse_args()
    verify(a.exe)
