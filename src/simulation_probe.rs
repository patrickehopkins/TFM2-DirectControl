//! Read-only entry/exit probe for known Teamfight Manager 2 client simulation jobs.
//!
//! Static analysis of TeamfightManager2.exe found three client-side functions in
//! `game-view/src/logic/client/data.rs` that call the common game-core simulation wrapper.
//! Candidate A has now been physically confirmed as the watched-match simulation job.
//! This module still detours A/B/C unchanged for timing, and additionally exposes the first
//! bytes of the common game-core wrapper/runner so the next inner-loop hook can be chosen
//! without guessing instruction boundaries.
//!
//! No simulation state or gameplay input is modified here.

use std::{
    ffi::c_void,
    fmt::Write as _,
    ptr,
    sync::{
        atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering},
        OnceLock,
    },
};

#[derive(Debug, Clone, Copy)]
struct SimulationLayout {
    pe_timestamp: u32,
    image_size: u32,
    candidate_rvas: [usize; 3],
    core_wrapper_rva: Option<usize>,
    /// v0.5.8 has a separate runner function. In v0.6.0 the equivalent body is
    /// inlined into the enlarged wrapper, so this is a verified body anchor.
    /// New builds leave these diagnostics absent until they are independently relocated.
    core_runner_anchor_rva: Option<usize>,
}

const BUILD_0_5_8: SimulationLayout = SimulationLayout {
    pe_timestamp: 0x6A97_8218,
    image_size: 0x04A1_D000,
    candidate_rvas: [0x00B1_CF10, 0x00B1_DB20, 0x00B1_E730],
    core_wrapper_rva: Some(0x0180_EAA0),
    core_runner_anchor_rva: Some(0x0181_3FB0),
};

const BUILD_0_6_0: SimulationLayout = SimulationLayout {
    pe_timestamp: 0x6AAA_07D1,
    image_size: 0x0522_8000,
    candidate_rvas: [0x00AC_2AE0, 0x00AC_36F0, 0x00AC_4300],
    core_wrapper_rva: Some(0x016D_2740),
    core_runner_anchor_rva: Some(0x016D_3880),
};

const BUILD_0_6_1: SimulationLayout = SimulationLayout {
    pe_timestamp: 0x6AB1_D950,
    image_size: 0x0526_4000,
    candidate_rvas: [0x00B8_CB20, 0x00B8_D730, 0x00B8_E340],
    // The .pdata relocation report strongly identifies the A/B/C job triple but leaves
    // two unusually large shared callees. These anchors are diagnostic-only, so do not
    // guess between them before a separate wrapper relocation proves their identity.
    core_wrapper_rva: None,
    core_runner_anchor_rva: None,
};

fn known_layout(timestamp: u32, image_size: u32) -> Option<&'static SimulationLayout> {
    [&BUILD_0_5_8, &BUILD_0_6_0, &BUILD_0_6_1]
        .into_iter()
        .find(|layout| layout.pe_timestamp == timestamp && layout.image_size == image_size)
}

const INSPECT_BYTES: usize = 32;

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

#[derive(Debug, Clone)]
pub struct CoreSignatureSnapshot {
    pub wrapper_rva: usize,
    pub wrapper_bytes: String,
    pub runner_rva: usize,
    pub runner_bytes: String,
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
        self.max_duration_ms
            .fetch_max(duration_ms, Ordering::Relaxed);
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
static ACTIVE_LAYOUT: OnceLock<&'static SimulationLayout> = OnceLock::new();
static INSTALL_RESULT: OnceLock<Result<(), String>> = OnceLock::new();

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
    let rvas = ACTIVE_LAYOUT
        .get()
        .copied()
        .unwrap_or(&BUILD_0_6_0)
        .candidate_rvas;
    [
        SLOT_A.snapshot("A:data.rs:6143", rvas[0]),
        SLOT_B.snapshot("B:data.rs:6560", rvas[1]),
        SLOT_C.snapshot("C:data.rs:4053", rvas[2]),
    ]
}

pub fn core_signatures() -> Result<CoreSignatureSnapshot, String> {
    unsafe {
        let module_base = GetModuleHandleW(ptr::null());
        if module_base.is_null() {
            return Err("GetModuleHandleW(NULL) failed".to_owned());
        }
        let base = module_base.cast::<u8>();
        let layout = ACTIVE_LAYOUT.get().copied().unwrap_or(&BUILD_0_6_0);
        let wrapper_rva = layout.core_wrapper_rva.ok_or_else(|| {
            "core simulation wrapper has not been independently relocated for this build".to_owned()
        })?;
        let runner_rva = layout.core_runner_anchor_rva.ok_or_else(|| {
            "core simulation runner anchor has not been independently relocated for this build".to_owned()
        })?;
        Ok(CoreSignatureSnapshot {
            wrapper_rva,
            wrapper_bytes: format_bytes(base.add(wrapper_rva), INSPECT_BYTES),
            runner_rva,
            runner_bytes: format_bytes(base.add(runner_rva), INSPECT_BYTES),
        })
    }
}

unsafe fn format_bytes(address: *const u8, count: usize) -> String {
    let bytes = std::slice::from_raw_parts(address, count);
    let mut out = String::with_capacity(count * 3);
    for (index, byte) in bytes.iter().enumerate() {
        if index != 0 {
            out.push(' ');
        }
        let _ = write!(&mut out, "{byte:02X}");
    }
    out
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
    let layout = known_layout(timestamp, image_size).ok_or_else(|| {
        format!(
            "unsupported TeamfightManager2.exe build (timestamp=0x{timestamp:08X}, image=0x{image_size:08X})"
        )
    })?;
    let _ = ACTIVE_LAYOUT.set(layout);

    for (name, rva) in ["A", "B", "C"].into_iter().zip(layout.candidate_rvas) {
        let target = base.add(rva);
        let actual = std::slice::from_raw_parts(target, PATCH_LEN);
        if actual != EXPECTED_PROLOGUE {
            return Err(format!(
                "simulation candidate {name} signature mismatch at RVA 0x{rva:X}"
            ));
        }
    }

    install_detour(
        base.add(layout.candidate_rvas[0]),
        hook_a as usize,
        &TRAMPOLINE_A,
    )?;
    install_detour(
        base.add(layout.candidate_rvas[1]),
        hook_b as usize,
        &TRAMPOLINE_B,
    )?;
    install_detour(
        base.add(layout.candidate_rvas[2]),
        hook_c as usize,
        &TRAMPOLINE_C,
    )?;
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
    let _ = VirtualProtect(
        target.cast::<c_void>(),
        PATCH_LEN,
        old_protect,
        &mut ignored,
    );
    let _ = FlushInstructionCache(GetCurrentProcess(), target.cast::<c_void>(), PATCH_LEN);
    Ok(())
}

unsafe fn write_abs_jump(destination: *mut u8, target: usize) {
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
