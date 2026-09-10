//! Read-only entry/exit probe for Teamfight Manager 2 v0.5.8 client simulation jobs.
//!
//! Static analysis of TeamfightManager2.exe found three client-side functions in
//! `game-view/src/logic/client/data.rs` that call the common game-core simulation wrapper.
//! Their surrounding closure wrappers strongly suggest background simulation jobs. This
//! module detours only those three function entries, calls the originals unchanged, and
//! records timing/thread metadata so we can identify which job produces the match that is
//! actually watched.
//!
//! No simulation state or gameplay input is modified here.

use std::{
    ffi::c_void,
    ptr,
    sync::{
        atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering},
        OnceLock,
    },
};

const EXPECTED_PE_TIMESTAMP: u32 = 0x6A97_8218;
const EXPECTED_IMAGE_SIZE: u32 = 0x04A1_D000;

// `game-view/src/logic/client/data.rs` candidates discovered by static analysis.
const CANDIDATE_A_RVA: usize = 0x00B1_CF10; // around source lines 6143-6149
const CANDIDATE_B_RVA: usize = 0x00B1_DB20; // around source lines 6560-6567
const CANDIDATE_C_RVA: usize = 0x00B1_E730; // around source lines 4053-4054

// All three candidates begin with the same eight whole push instructions. Keeping the patch
// entirely inside whole non-RIP-relative instructions makes each trampoline relocatable.
const PATCH_LEN: usize = 12;
const EXPECTED_PROLOGUE: [u8; PATCH_LEN] = [
    0x55, 0x41, 0x57, 0x41, 0x56, 0x41, 0x55, 0x41, 0x54, 0x56, 0x57, 0x53,
];
const ABS_JUMP_LEN: usize = 12;
const TRAMPOLINE_LEN: usize = PATCH_LEN + ABS_JUMP_LEN;

const MEM_COMMIT: u32 = 0x1000;
const MEM_RESERVE: u32 = 0x2000;
const PAGE_EXECUTE_READWRITE: u32 = 0x40;

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(module_name: *const u16) -> *mut c_void;
    fn GetCurrentProcess() -> *mut c_void;
    fn GetCurrentThreadId() -> u32;
    fn GetTickCount64() -> u64;
    fn VirtualAlloc(
        address: *mut c_void,
        size: usize,
        allocation_type: u32,
        protect: u32,
    ) -> *mut c_void;
    fn VirtualProtect(
        address: *mut c_void,
        size: usize,
        new_protect: u32,
        old_protect: *mut u32,
    ) -> i32;
    fn FlushInstructionCache(process: *mut c_void, address: *const c_void, size: usize) -> i32;
}

#[derive(Debug, Clone, Copy)]
pub struct SimulationProbeSnapshot {
    pub name: &'static str,
    pub rva: usize,
    pub entries: u64,
    pub active: u32,
    pub completions: u64,
    pub last_thread_id: u32,
    pub last_context: usize,
    pub last_duration_ms: u64,
    pub max_duration_ms: u64,
}

struct ProbeSlot {
    entries: AtomicU64,
    active: AtomicU32,
    completions: AtomicU64,
    last_thread_id: AtomicU32,
    last_context: AtomicUsize,
    last_duration_ms: AtomicU64,
    max_duration_ms: AtomicU64,
}

impl ProbeSlot {
    const fn new() -> Self {
        Self {
            entries: AtomicU64::new(0),
            active: AtomicU32::new(0),
            completions: AtomicU64::new(0),
            last_thread_id: AtomicU32::new(0),
            last_context: AtomicUsize::new(0),
            last_duration_ms: AtomicU64::new(0),
            max_duration_ms: AtomicU64::new(0),
        }
    }

    fn enter(&self, context: *mut u8) -> u64 {
        self.entries.fetch_add(1, Ordering::Relaxed);
        self.active.fetch_add(1, Ordering::AcqRel);
        self.last_thread_id
            .store(unsafe { GetCurrentThreadId() }, Ordering::Relaxed);
        self.last_context.store(context as usize, Ordering::Relaxed);
        unsafe { GetTickCount64() }
    }

    fn exit(&self, start_ms: u64) {
        let duration_ms = unsafe { GetTickCount64() }.saturating_sub(start_ms);
        self.last_duration_ms.store(duration_ms, Ordering::Relaxed);
        self.max_duration_ms.fetch_max(duration_ms, Ordering::Relaxed);
        self.completions.fetch_add(1, Ordering::Relaxed);
        self.active.fetch_sub(1, Ordering::AcqRel);
    }

    fn snapshot(&self, name: &'static str, rva: usize) -> SimulationProbeSnapshot {
        SimulationProbeSnapshot {
            name,
            rva,
            entries: self.entries.load(Ordering::Acquire),
            active: self.active.load(Ordering::Acquire),
            completions: self.completions.load(Ordering::Acquire),
            last_thread_id: self.last_thread_id.load(Ordering::Acquire),
            last_context: self.last_context.load(Ordering::Acquire),
            last_duration_ms: self.last_duration_ms.load(Ordering::Acquire),
            max_duration_ms: self.max_duration_ms.load(Ordering::Acquire),
        }
    }
}

static SLOT_A: ProbeSlot = ProbeSlot::new();
static SLOT_B: ProbeSlot = ProbeSlot::new();
static SLOT_C: ProbeSlot = ProbeSlot::new();

static TRAMPOLINE_A: AtomicUsize = AtomicUsize::new(0);
static TRAMPOLINE_B: AtomicUsize = AtomicUsize::new(0);
static TRAMPOLINE_C: AtomicUsize = AtomicUsize::new(0);
static INSTALL_RESULT: OnceLock<Result<(), String>> = OnceLock::new();

// Each candidate is called by its closure wrapper with RCX pointing to one captured context
// object; the wrappers do not consume a return value.
type SimulationJobFn = unsafe extern "system" fn(*mut u8);

unsafe extern "system" fn hook_a(context: *mut u8) {
    run_hook(context, &SLOT_A, &TRAMPOLINE_A);
}

unsafe extern "system" fn hook_b(context: *mut u8) {
    run_hook(context, &SLOT_B, &TRAMPOLINE_B);
}

unsafe extern "system" fn hook_c(context: *mut u8) {
    run_hook(context, &SLOT_C, &TRAMPOLINE_C);
}

unsafe fn run_hook(context: *mut u8, slot: &ProbeSlot, trampoline: &AtomicUsize) {
    let started = slot.enter(context);
    let original_address = trampoline.load(Ordering::Acquire);
    if original_address != 0 {
        let original: SimulationJobFn = std::mem::transmute(original_address);
        original(context);
    }
    slot.exit(started);
}

pub fn snapshots() -> [SimulationProbeSnapshot; 3] {
    [
        SLOT_A.snapshot("A:data.rs:6143", CANDIDATE_A_RVA),
        SLOT_B.snapshot("B:data.rs:6560", CANDIDATE_B_RVA),
        SLOT_C.snapshot("C:data.rs:4053", CANDIDATE_C_RVA),
    ]
}

pub fn ensure_installed() -> Result<(), String> {
    INSTALL_RESULT
        .get_or_init(|| unsafe { install_inner() })
        .clone()
}

unsafe fn install_inner() -> Result<(), String> {
    let module_base = GetModuleHandleW(ptr::null());
    if module_base.is_null() {
        return Err("GetModuleHandleW(NULL) failed".to_owned());
    }

    let base = module_base.cast::<u8>();
    let pe_offset = ptr::read_unaligned(base.add(0x3c).cast::<u32>()) as usize;
    if ptr::read_unaligned(base.add(pe_offset).cast::<u32>()) != 0x0000_4550 {
        return Err("main module does not contain a valid PE signature".to_owned());
    }

    let timestamp = ptr::read_unaligned(base.add(pe_offset + 8).cast::<u32>());
    let optional_header = pe_offset + 24;
    let image_size = ptr::read_unaligned(base.add(optional_header + 56).cast::<u32>());
    if timestamp != EXPECTED_PE_TIMESTAMP || image_size != EXPECTED_IMAGE_SIZE {
        return Err(format!(
            "unsupported TeamfightManager2.exe build (timestamp=0x{timestamp:08X}, image=0x{image_size:08X})"
        ));
    }

    // Validate every target before changing any executable bytes.
    for (name, rva) in [
        ("A", CANDIDATE_A_RVA),
        ("B", CANDIDATE_B_RVA),
        ("C", CANDIDATE_C_RVA),
    ] {
        let target = base.add(rva);
        let actual = std::slice::from_raw_parts(target, PATCH_LEN);
        if actual != EXPECTED_PROLOGUE {
            return Err(format!(
                "simulation candidate {name} signature mismatch at RVA 0x{rva:X}"
            ));
        }
    }

    install_detour(base.add(CANDIDATE_A_RVA), hook_a as usize, &TRAMPOLINE_A)?;
    install_detour(base.add(CANDIDATE_B_RVA), hook_b as usize, &TRAMPOLINE_B)?;
    install_detour(base.add(CANDIDATE_C_RVA), hook_c as usize, &TRAMPOLINE_C)?;

    Ok(())
}

unsafe fn install_detour(
    target: *mut u8,
    hook: usize,
    trampoline_slot: &AtomicUsize,
) -> Result<(), String> {
    let trampoline = VirtualAlloc(
        ptr::null_mut(),
        TRAMPOLINE_LEN,
        MEM_COMMIT | MEM_RESERVE,
        PAGE_EXECUTE_READWRITE,
    )
    .cast::<u8>();
    if trampoline.is_null() {
        return Err("VirtualAlloc failed while creating simulation trampoline".to_owned());
    }

    ptr::copy_nonoverlapping(target, trampoline, PATCH_LEN);
    write_abs_jump(trampoline.add(PATCH_LEN), target.add(PATCH_LEN) as usize);
    trampoline_slot.store(trampoline as usize, Ordering::Release);

    let mut old_protect = 0u32;
    if VirtualProtect(
        target.cast::<c_void>(),
        PATCH_LEN,
        PAGE_EXECUTE_READWRITE,
        &mut old_protect,
    ) == 0
    {
        trampoline_slot.store(0, Ordering::Release);
        return Err("VirtualProtect failed while enabling simulation detour write".to_owned());
    }

    write_abs_jump(target, hook);

    let mut ignored = 0u32;
    let _ = VirtualProtect(target.cast::<c_void>(), PATCH_LEN, old_protect, &mut ignored);
    let _ = FlushInstructionCache(GetCurrentProcess(), target.cast::<c_void>(), PATCH_LEN);

    Ok(())
}

unsafe fn write_abs_jump(destination: *mut u8, target: usize) {
    // mov rax, imm64 ; jmp rax
    *destination = 0x48;
    *destination.add(1) = 0xB8;
    ptr::copy_nonoverlapping(
        target.to_le_bytes().as_ptr(),
        destination.add(2),
        std::mem::size_of::<usize>(),
    );
    *destination.add(10) = 0xFF;
    *destination.add(11) = 0xE0;
}
