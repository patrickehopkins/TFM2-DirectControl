//! Binding-independent native replay shortcut gate for verified TFM2 builds.
//!
//! This detours the *runtime action -> current key* lookup, NOT action-name
//! formatting, the physical keyboard, or persisted shortcut configuration.
//! The watched-match camera handler invokes this lookup for all replay seek
//! actions before comparing the returned key with the incoming native event.
//! Returning the intentionally unassigned key 0xFF makes those comparisons
//! fail regardless of how the user remapped their shortcuts.
//!
//! The upper-left replay UI and bottom Highlight UI remain separately hidden
//! through the stable client API. Native camera actions 0x35/0x36 and all
//! non-replay actions are ALWAYS forwarded unchanged.
//!
//! Every RVA/signature is selected only for an exact verified PE build.
//! An unsupported game must not silently enter live control unprotected.

use std::{
    ffi::c_void,
    ptr,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        OnceLock,
    },
};

#[derive(Debug, Clone, Copy)]
struct ReplayActionLayout {
    pe_timestamp: u32,
    image_size: u32,
    binding_lookup_rva: usize,
}

const BUILD_0_6_1: ReplayActionLayout = ReplayActionLayout {
    pe_timestamp: 0x6AB1_D950,
    image_size: 0x0526_4000,
    binding_lookup_rva: 0x021C_4CE0,
};

const BUILD_0_6_2: ReplayActionLayout = ReplayActionLayout {
    pe_timestamp: 0x6ABC_597E,
    image_size: 0x052B_8000,
    binding_lookup_rva: 0x028D_3E90,
};

fn known_layout(timestamp: u32, image_size: u32) -> Option<&'static ReplayActionLayout> {
    [&BUILD_0_6_1, &BUILD_0_6_2]
        .into_iter()
        .find(|layout| layout.pe_timestamp == timestamp && layout.image_size == image_size)
}

// Exact first 12 complete instructions bytes, verified from the uploaded
// SHA-256 91084e9a...d15c2268f98 executable. No RIP-relative instructions;
// the next instruction after these 12 bytes reads the original rcx.
const PATCH_LEN: usize = 12;
const EXPECTED_PROLOGUE: [u8; PATCH_LEN] = [
    0x56, // push rsi
    0x53, // push rbx
    0x48, 0x83, 0xEC, 0x28, // sub rsp, 0x28
    0x89, 0xD3, // mov ebx, edx
    0x88, 0x54, 0x24, 0x27, // mov [rsp+0x27], dl
];
const ABS_JUMP_LEN: usize = 12;
const UNBOUND_KEY: u8 = 0xFF;
const MEM_COMMIT: u32 = 0x1000;
const MEM_RESERVE: u32 = 0x2000;
const MEM_RELEASE: u32 = 0x8000;
const PAGE_EXECUTE_READWRITE: u32 = 0x40;

type NativeBindingLookup = unsafe extern "system" fn(*const c_void, u32) -> u8;

static INSTALL_RESULT: OnceLock<Result<(), String>> = OnceLock::new();
static TRAMPOLINE: AtomicUsize = AtomicUsize::new(0);
static MATCH_OWNED: AtomicBool = AtomicBool::new(false);
static BLOCKED_LOOKUPS: AtomicU64 = AtomicU64::new(0);

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(module_name: *const u16) -> *mut c_void;
    fn GetCurrentProcess() -> *mut c_void;
    fn VirtualAlloc(
        address: *mut c_void,
        size: usize,
        allocation_type: u32,
        protect: u32,
    ) -> *mut c_void;
    fn VirtualFree(address: *mut c_void, size: usize, free_type: u32) -> i32;
    fn VirtualProtect(
        address: *mut c_void,
        size: usize,
        new_protect: u32,
        old_protect: *mut u32,
    ) -> i32;
    fn FlushInstructionCache(process: *mut c_void, address: *const c_void, size: usize) -> i32;
}

fn forbidden_replay_action(action: u8) -> bool {
    matches!(
        action,
        0x1B // Highlight playback mode
            | 0x30 // Previous highlight
            | 0x31 // Back ten seconds
            | 0x32 // Native timeline pause (NOT the separately synchronized pause menu)
            | 0x33 // Forward ten seconds
            | 0x34 // Next highlight
    )
}

unsafe extern "system" fn binding_lookup_hook(bindings: *const c_void, action: u32) -> u8 {
    // The release flag is checked directly here so the original binding becomes
    // available immediately after confirmed Ctrl+End, not one render frame later.
    if MATCH_OWNED.load(Ordering::Acquire)
        && !crate::pacing_probe::manual_control_released()
        && forbidden_replay_action(action as u8)
    {
        BLOCKED_LOOKUPS.fetch_add(1, Ordering::Relaxed);
        return UNBOUND_KEY;
    }

    let trampoline = TRAMPOLINE.load(Ordering::Acquire);
    // The trampoline is installed before the detour becomes reachable. This
    // branch should be unreachable but avoid jumping through null if violated.
    if trampoline == 0 {
        return UNBOUND_KEY;
    }
    let original: NativeBindingLookup = std::mem::transmute(trampoline);
    original(bindings, action)
}

unsafe fn write_absolute_jump(destination: *mut u8, target: usize) {
    // mov rax, absolute target; jmp rax. The original bytes following our
    // trampoline prologue do not depend on the prior value of rax.
    destination.write(0x48);
    destination.add(1).write(0xB8);
    ptr::copy_nonoverlapping(target.to_le_bytes().as_ptr(), destination.add(2), 8);
    destination.add(10).write(0xFF);
    destination.add(11).write(0xE0);
}

unsafe fn install_inner() -> Result<(), String> {
    let module = GetModuleHandleW(ptr::null()).cast::<u8>();
    if module.is_null() {
        return Err("GetModuleHandleW(NULL) failed".to_owned());
    }
    if ptr::read_unaligned(module.cast::<u16>()) != 0x5A4D {
        return Err("main executable MZ signature mismatch".to_owned());
    }
    let pe_offset = ptr::read_unaligned(module.add(0x3C).cast::<u32>()) as usize;
    if ptr::read_unaligned(module.add(pe_offset).cast::<u32>()) != 0x0000_4550 {
        return Err("main executable PE signature mismatch".to_owned());
    }
    let timestamp = ptr::read_unaligned(module.add(pe_offset + 8).cast::<u32>());
    let image_size = ptr::read_unaligned(module.add(pe_offset + 24 + 56).cast::<u32>());
    let layout = known_layout(timestamp, image_size).ok_or_else(|| {
        format!(
            "unsupported replay action lookup build: timestamp=0x{timestamp:08X}, image=0x{image_size:08X}"
        )
    })?;

    let target = module.add(layout.binding_lookup_rva);
    if std::slice::from_raw_parts(target, PATCH_LEN) != EXPECTED_PROLOGUE {
        return Err(format!(
            "replay binding lookup signature mismatch at RVA 0x{:X}",
            layout.binding_lookup_rva
        ));
    }

    let trampoline = VirtualAlloc(
        ptr::null_mut(),
        PATCH_LEN + ABS_JUMP_LEN,
        MEM_COMMIT | MEM_RESERVE,
        PAGE_EXECUTE_READWRITE,
    )
    .cast::<u8>();
    if trampoline.is_null() {
        return Err("VirtualAlloc failed for replay binding lookup trampoline".to_owned());
    }
    ptr::copy_nonoverlapping(target, trampoline, PATCH_LEN);
    write_absolute_jump(trampoline.add(PATCH_LEN), target.add(PATCH_LEN) as usize);

    TRAMPOLINE.store(trampoline as usize, Ordering::Release);
    let mut previous_protection = 0u32;
    if VirtualProtect(
        target.cast(),
        PATCH_LEN,
        PAGE_EXECUTE_READWRITE,
        &mut previous_protection,
    ) == 0
    {
        TRAMPOLINE.store(0, Ordering::Release);
        let _ = VirtualFree(trampoline.cast(), 0, MEM_RELEASE);
        return Err("VirtualProtect refused replay binding lookup detour".to_owned());
    }

    // Installed on module initialization, before the watched match starts.
    write_absolute_jump(target, binding_lookup_hook as usize);

    let mut ignored_protection = 0u32;
    let restored = VirtualProtect(
        target.cast(),
        PATCH_LEN,
        previous_protection,
        &mut ignored_protection,
    );
    let flushed = FlushInstructionCache(GetCurrentProcess(), target.cast(), PATCH_LEN);
    if restored == 0 || flushed == 0 {
        // The hook is already live. Refuse manual control instead of claiming
        // an installation whose memory protection/cache update could not be verified.
        return Err("replay hook patched but VirtualProtect/cache flush failed; Direct Control start blocked".to_owned());
    }
    Ok(())
}

pub fn install() -> Result<(), String> {
    INSTALL_RESULT
        .get_or_init(|| unsafe { install_inner() })
        .clone()
}

pub fn installed() -> bool {
    matches!(INSTALL_RESULT.get(), Some(Ok(())))
}

pub fn set_match_owned(owned: bool) {
    MATCH_OWNED.store(owned, Ordering::Release);
}

#[cfg(feature = "replay-native-trace")]
pub fn blocked_lookup_count() -> u64 {
    BLOCKED_LOOKUPS.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_replay_navigation_is_rejected() {
        for id in [0x1B, 0x30, 0x31, 0x32, 0x33, 0x34] {
            assert!(forbidden_replay_action(id));
        }
        for id in [0x00, 0x22, 0x2C, 0x2D, 0x35, 0x36, 0xFF] {
            assert!(!forbidden_replay_action(id));
        }
    }
}
