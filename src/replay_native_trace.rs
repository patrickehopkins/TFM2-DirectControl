//! Opt-in, observational v0.6.1 replay action-lookup trace.
//!
//! This is NOT seek suppression. It records which native code calls the verified
//! action-enum hasher for replay/navigation actions, including while M/6 is held.
//! The native hook patches its target but never modifies game input or the returned hash.
//! It is a temporary diagnostic only. Keep it feature gated;
//! never ship the diagnostic in the Workshop build.
//!
//! Enable with: .\scripts\install-dev.ps1 -ReplayTrace
//! In a match after Ctrl+End: Ctrl+Alt+F12, click Back 10 Seconds, press M,
//! press 6, Ctrl+Alt+F12. Report: %TEMP%\tfm2_replay_native_trace.txt

use std::{
    ffi::c_void,
    fmt::Write as _,
    ptr,
    sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};

use mod_api_stable::{ClientSceneKindV1, StableClient};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_F12, VK_MENU,
};

use crate::input_focus;

const PE_TIMESTAMP: u32 = 0x6AB1_D950;
const PE_IMAGE_SIZE: u32 = 0x0526_4000;
const HASH_RVA: usize = 0x00BA_3A30;

// Three complete instructions: sub rsp,78; movdqu xmm0,[rcx]; pshufd xmm1,xmm0,44.
// No RIP-relative instruction is copied into the trampoline.
const PATCH_LEN: usize = 13;
const HASH_PROLOGUE: [u8; PATCH_LEN] = [
    0x48, 0x83, 0xEC, 0x78, 0xF3, 0x0F, 0x6F, 0x01,
    0x66, 0x0F, 0x70, 0xC8, 0x44,
];
const MEM_COMMIT: u32 = 0x1000;
const MEM_RESERVE: u32 = 0x2000;
const PAGE_EXECUTE_READWRITE: u32 = 0x40;
const ABS_JUMP_LEN: usize = 12;
const MAX_UNIQUE_SAMPLES: usize = 256;

type NativeHash = unsafe extern "system" fn(*const u8, *const u8) -> u64;

static MAIN_BASE: AtomicUsize = AtomicUsize::new(0);
static TRAMPOLINE: AtomicUsize = AtomicUsize::new(0);
static TRACE_ACTIVE: AtomicBool = AtomicBool::new(false);
static TRACE_CHORD_WAS_DOWN: AtomicBool = AtomicBool::new(false);
static TRACE_SLOTS: [AtomicU64; MAX_UNIQUE_SAMPLES] =
    [const { AtomicU64::new(0) }; MAX_UNIQUE_SAMPLES];
static TRACE_DROPPED: AtomicU64 = AtomicU64::new(0);
static BASELINE_BUDGET: AtomicUsize = AtomicUsize::new(0);
static M_BUDGET: AtomicUsize = AtomicUsize::new(0);
static SIX_BUDGET: AtomicUsize = AtomicUsize::new(0);

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    fn GetCurrentProcess() -> *mut c_void;
    fn VirtualAlloc(
        address: *mut c_void,
        size: usize,
        allocation_type: u32,
        protection: u32,
    ) -> *mut c_void;
    fn VirtualProtect(
        address: *mut c_void,
        size: usize,
        new_protection: u32,
        previous_protection: *mut u32,
    ) -> i32;
    fn FlushInstructionCache(process: *mut c_void, address: *const c_void, size: usize) -> i32;
    fn RtlCaptureStackBackTrace(
        skip: u32,
        capture: u32,
        frames: *mut *mut c_void,
        hash: *mut u32,
    ) -> u16;
}

unsafe fn write_absolute_jump(destination: *mut u8, target: usize) {
    destination.write(0x48);
    destination.add(1).write(0xB8); // mov rax, absolute target
    ptr::copy_nonoverlapping(target.to_le_bytes().as_ptr(), destination.add(2), 8);
    destination.add(10).write(0xFF);
    destination.add(11).write(0xE0); // jmp rax
}

fn traced_action(id: u8) -> bool {
    matches!(id, 0x1B | 0x30 | 0x31 | 0x32 | 0x33 | 0x34)
}

fn test_key_flags() -> u8 {
    // Physical keys annotate this one diagnostic run. Real suppression MUST use
    // semantic native actions; no keyboard blacklist is being implemented.
    unsafe {
        (u8::from(GetAsyncKeyState(b'M' as i32) < 0))
            | (u8::from(GetAsyncKeyState(b'6' as i32) < 0) << 1)
    }
}

fn record_sample(id: u8, keys: u8) {
    // Avoid expensive stack unwinds on every frame of a hot native lookup.
    // Reserve separate budgets so the baseline cannot exhaust M/6 observations.
    let budget = if keys & 1 != 0 {
        &M_BUDGET
    } else if keys & 2 != 0 {
        &SIX_BUDGET
    } else {
        &BASELINE_BUDGET
    };
    if budget.fetch_add(1, Ordering::Relaxed) >= 128 {
        return;
    }

    let mut frames = [ptr::null_mut(); 4];
    let count = unsafe {
        RtlCaptureStackBackTrace(0, frames.len() as u32, frames.as_mut_ptr(), ptr::null_mut())
    } as usize;
    let base = MAIN_BASE.load(Ordering::Acquire);
    let caller = frames[..count]
        .iter()
        .map(|p| *p as usize)
        .find(|&address| address >= base && address < base + PE_IMAGE_SIZE as usize)
        .map(|address| address - base)
        .unwrap_or(0);

    // Nonzero encoding; identical hashed-byte/callsite/key-state samples are deduplicated.
    let value = ((id as u64) << 56) | ((keys as u64) << 48) | caller as u64;
    let encoded = value.wrapping_add(1);
    for slot in TRACE_SLOTS.iter() {
        let old = slot.load(Ordering::Acquire);
        if old == encoded {
            return;
        }
        if old == 0 {
            if slot.compare_exchange(0, encoded, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                return;
            }
        }
    }
    TRACE_DROPPED.fetch_add(1, Ordering::Relaxed);
}

unsafe extern "system" fn hash_hook(hasher: *const u8, action: *const u8) -> u64 {
    // This hook must remain transparent: never change action, hash state, return value,
    // or game event flow. Action data and code generation have been confirmed for
    // the one executable above; other versions never reach this hook.
    if TRACE_ACTIVE.load(Ordering::Relaxed) && !action.is_null() {
        let id = action.read();
        let keys = test_key_flags();
        // The hasher is shared by both semantic Action and physical Key enums.
        // Always observe the interesting action-like bytes, but while M/6 is
        // physically held also sample *all* hashed bytes to catch keyboard
        // dispatch even when Key::M/Key::6 has a different enum discriminant.
        if traced_action(id) || keys != 0 {
            record_sample(id, keys);
        }
    }
    let original: NativeHash = std::mem::transmute(TRAMPOLINE.load(Ordering::Acquire));
    original(hasher, action)
}

pub fn install() -> Result<(), String> {
    if TRAMPOLINE.load(Ordering::Acquire) != 0 {
        return Ok(());
    }

    unsafe {
        let base = GetModuleHandleW(ptr::null());
        if base.is_null() {
            return Err("GetModuleHandleW(NULL) failed".into());
        }
        let module = base.cast::<u8>();
        if ptr::read_unaligned(module.cast::<u16>()) != 0x5A4D {
            return Err("main module is not an MZ image".into());
        }
        let pe_offset = ptr::read_unaligned(module.add(0x3C).cast::<u32>()) as usize;
        if ptr::read_unaligned(module.add(pe_offset).cast::<u32>()) != 0x0000_4550 {
            return Err("main module PE signature mismatch".into());
        }
        let timestamp = ptr::read_unaligned(module.add(pe_offset + 8).cast::<u32>());
        let image_size = ptr::read_unaligned(module.add(pe_offset + 24 + 56).cast::<u32>());
        if timestamp != PE_TIMESTAMP || image_size != PE_IMAGE_SIZE {
            return Err(format!(
                "unsupported build timestamp=0x{timestamp:08X}, image=0x{image_size:08X}"
            ));
        }
        let target = module.add(HASH_RVA);
        if std::slice::from_raw_parts(target, PATCH_LEN) != &HASH_PROLOGUE[..] {
            return Err("native action hasher prologue mismatch; trace not installed".into());
        }

        let trampoline = VirtualAlloc(
            ptr::null_mut(),
            PATCH_LEN + ABS_JUMP_LEN,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_EXECUTE_READWRITE,
        );
        if trampoline.is_null() {
            return Err("VirtualAlloc for replay trace trampoline failed".into());
        }
        ptr::copy_nonoverlapping(target, trampoline.cast::<u8>(), PATCH_LEN);
        write_absolute_jump(
            trampoline.cast::<u8>().add(PATCH_LEN),
            target.add(PATCH_LEN) as usize,
        );
        MAIN_BASE.store(base as usize, Ordering::Release);
        TRAMPOLINE.store(trampoline as usize, Ordering::Release);

        let mut previous_protection = 0u32;
        if VirtualProtect(
            target.cast(),
            PATCH_LEN,
            PAGE_EXECUTE_READWRITE,
            &mut previous_protection,
        ) == 0 {
            TRAMPOLINE.store(0, Ordering::Release);
            return Err("VirtualProtect refused replay trace patch".into());
        }
        write_absolute_jump(target, hash_hook as usize);
        target.add(ABS_JUMP_LEN).write(0x90);
        let _ = FlushInstructionCache(GetCurrentProcess(), target.cast(), PATCH_LEN);

        let mut ignored_protection = 0u32;
        let _ = VirtualProtect(
            target.cast(),
            PATCH_LEN,
            previous_protection,
            &mut ignored_protection,
        );
    }
    Ok(())
}

fn reset_samples() {
    TRACE_ACTIVE.store(false, Ordering::Release);
    TRACE_DROPPED.store(0, Ordering::Release);
    BASELINE_BUDGET.store(0, Ordering::Release);
    M_BUDGET.store(0, Ordering::Release);
    SIX_BUDGET.store(0, Ordering::Release);
    for slot in TRACE_SLOTS.iter() {
        slot.store(0, Ordering::Release);
    }
}

fn write_report() {
    let mut out = String::from(
        "TFM2 v0.6.1 native replay action/key-hash diagnostic\n\n\
         Hasher is shared: hashed_byte MAY be a physical Key ID, NOT necessarily a semantic Action.\n\
         Interesting action IDs when used as Action: 0x1B Highlight mode;\n\
         0x30 Previous Highlight; 0x31 Back 10 Seconds; 0x32 Pause;\n\
         0x33 Forward 10 Seconds; 0x34 Next Highlight.\n\
         Key flags: 0=no M/6 held, 1=M held, 2=6 held, 3=both held.\n\
         RVA 0 means stack capture recovered no main executable frame.\n\n",
    );
    for slot in TRACE_SLOTS.iter() {
        let encoded = slot.load(Ordering::Acquire);
        if encoded == 0 {
            continue;
        }
        let value = encoded.wrapping_sub(1);
        let id = (value >> 56) as u8;
        let keys = ((value >> 48) & 0xFF) as u8;
        let rva = value as u32;
        let _ = writeln!(out, "hashed_byte=0x{id:02X} keys={keys} caller_rva=0x{rva:08X}");
    }
    let _ = writeln!(
        out,
        "\nUnique slots exhausted/dropped: {}",
        TRACE_DROPPED.load(Ordering::Acquire)
    );
    let _ = std::fs::write(std::env::temp_dir().join("tfm2_replay_native_trace.txt"), out);
}

pub fn poll_hotkey_and_dump(ctx: &StableClient<'_>) {
    let ingame = matches!(ctx.client_scene_kind(), Some(ClientSceneKindV1::InGame));
    if !ingame {
        TRACE_CHORD_WAS_DOWN.store(false, Ordering::Release);
        return;
    }
    if !input_focus::process_owns_foreground_window() {
        TRACE_CHORD_WAS_DOWN.store(true, Ordering::Release);
        return;
    }
    let chord_down = unsafe {
        GetAsyncKeyState(VK_CONTROL as i32) < 0
            && GetAsyncKeyState(VK_MENU as i32) < 0
            && GetAsyncKeyState(VK_F12 as i32) < 0
    };
    let was_down = TRACE_CHORD_WAS_DOWN.swap(chord_down, Ordering::AcqRel);
    if !chord_down || was_down {
        return;
    }
    if TRACE_ACTIVE.swap(false, Ordering::AcqRel) {
        write_report();
    } else {
        reset_samples();
        TRACE_ACTIVE.store(true, Ordering::Release);
    }
}
