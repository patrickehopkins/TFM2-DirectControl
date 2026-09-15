//! Version-checked camera-state capture for Teamfight Manager 2 v0.5.8.
//!
//! The official stable mod API deliberately does not expose the spectator camera.
//! For the single tested 0.5.8 executable we therefore detour the game's camera
//! input/update handler and capture only a few verified fields from its `this`
//! pointer. No gameplay mutation happens here.
//!
//! Keep every version-specific RVA/offset in this module. Higher-level direct
//! control code should consume `CameraSnapshot` and never know TFM2's private
//! object layout.

use std::{
    ffi::c_void,
    ptr,
    sync::{
        atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering},
        OnceLock,
    },
};

const HANDLER_RVA: usize = 0x009E_6750;
const EXPECTED_PE_TIMESTAMP: u32 = 0x6A97_8218;
const EXPECTED_IMAGE_SIZE: u32 = 0x04A1_D000;

const ZOOM_OFFSET: usize = 0xE0;
const CENTER_X_OFFSET: usize = 0xE4;
const CENTER_Y_OFFSET: usize = 0xE8;
const EXTENT_A_OFFSET: usize = 0xEC;
const EXTENT_B_OFFSET: usize = 0xF0;
const MODE_OFFSET: usize = 0xF4;

// The first 12 bytes of the confirmed handler are eight whole push instructions,
// so they can be copied to a trampoline without relocating RIP-relative code.
const PATCH_LEN: usize = 12;
const EXPECTED_PROLOGUE: [u8; PATCH_LEN] = [
    0x55, 0x41, 0x57, 0x41, 0x56, 0x41, 0x55, 0x41, 0x54, 0x56, 0x57, 0x53,
];
const ABS_JUMP_LEN: usize = 12;
const TRAMPOLINE_LEN: usize = PATCH_LEN + ABS_JUMP_LEN;
const MAX_CANDIDATES: usize = 4;

const MEM_COMMIT: u32 = 0x1000;
const MEM_RESERVE: u32 = 0x2000;
const PAGE_EXECUTE_READWRITE: u32 = 0x40;

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
    fn VirtualProtect(
        address: *mut c_void,
        size: usize,
        new_protect: u32,
        old_protect: *mut u32,
    ) -> i32;
    fn FlushInstructionCache(process: *mut c_void, address: *const c_void, size: usize) -> i32;
}

#[derive(Debug, Clone, Copy)]
pub struct CameraSnapshot {
    pub address: usize,
    pub zoom: f32,
    pub center_x: f32,
    pub center_y: f32,
    pub extent_a: f32,
    pub extent_b: f32,
    pub mode: u8,
    pub calls: u64,
}

struct CandidateSlot {
    address: AtomicUsize,
    zoom: AtomicU32,
    center_x: AtomicU32,
    center_y: AtomicU32,
    extent_a: AtomicU32,
    extent_b: AtomicU32,
    mode: AtomicU32,
    calls: AtomicU64,
}

impl CandidateSlot {
    const fn new() -> Self {
        Self {
            address: AtomicUsize::new(0),
            zoom: AtomicU32::new(0),
            center_x: AtomicU32::new(0),
            center_y: AtomicU32::new(0),
            extent_a: AtomicU32::new(0),
            extent_b: AtomicU32::new(0),
            mode: AtomicU32::new(0),
            calls: AtomicU64::new(0),
        }
    }

    fn clear(&self) {
        self.calls.store(0, Ordering::Relaxed);
        self.mode.store(0, Ordering::Relaxed);
        self.extent_b.store(0, Ordering::Relaxed);
        self.extent_a.store(0, Ordering::Relaxed);
        self.center_y.store(0, Ordering::Relaxed);
        self.center_x.store(0, Ordering::Relaxed);
        self.zoom.store(0, Ordering::Relaxed);
        self.address.store(0, Ordering::Release);
    }
}

static CANDIDATES: [CandidateSlot; MAX_CANDIDATES] = [
    CandidateSlot::new(),
    CandidateSlot::new(),
    CandidateSlot::new(),
    CandidateSlot::new(),
];
static TRAMPOLINE: AtomicUsize = AtomicUsize::new(0);
static INSTALL_RESULT: OnceLock<Result<(), String>> = OnceLock::new();

// Observed machine-level signature at the two known call sites. Only the first
// argument matters to us; preserving the rest exactly lets the original handler
// continue normally through the trampoline.
type CameraHandlerFn = unsafe extern "system" fn(
    *mut u8,
    usize,
    usize,
    f32,
    usize,
    usize,
    usize,
);

unsafe extern "system" fn camera_handler_hook(
    this: *mut u8,
    arg2: usize,
    arg3: usize,
    arg4: f32,
    arg5: usize,
    arg6: usize,
    arg7: usize,
) {
    let trampoline = TRAMPOLINE.load(Ordering::Acquire);
    if trampoline != 0 {
        let original: CameraHandlerFn = std::mem::transmute(trampoline);
        original(this, arg2, arg3, arg4, arg5, arg6, arg7);
    }

    capture(this);
}

unsafe fn capture(this: *mut u8) {
    if this.is_null() {
        return;
    }

    let zoom = ptr::read_unaligned(this.add(ZOOM_OFFSET).cast::<f32>());
    let center_x = ptr::read_unaligned(this.add(CENTER_X_OFFSET).cast::<f32>());
    let center_y = ptr::read_unaligned(this.add(CENTER_Y_OFFSET).cast::<f32>());
    let extent_a = ptr::read_unaligned(this.add(EXTENT_A_OFFSET).cast::<f32>());
    let extent_b = ptr::read_unaligned(this.add(EXTENT_B_OFFSET).cast::<f32>());
    let mode = ptr::read_unaligned(this.add(MODE_OFFSET).cast::<u8>());

    // Do not publish clearly nonsensical values if the handler layout ever changes.
    if !zoom.is_finite()
        || !center_x.is_finite()
        || !center_y.is_finite()
        || !extent_a.is_finite()
        || !extent_b.is_finite()
        || !(0.0..=4.0).contains(&zoom)
    {
        return;
    }

    let address = this as usize;
    let mut selected = None;

    for (index, slot) in CANDIDATES.iter().enumerate() {
        if slot.address.load(Ordering::Acquire) == address {
            selected = Some(index);
            break;
        }
    }

    if selected.is_none() {
        for (index, slot) in CANDIDATES.iter().enumerate() {
            if slot
                .address
                .compare_exchange(0, address, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                selected = Some(index);
                break;
            }
        }
    }

    let Some(index) = selected else {
        return;
    };
    let slot = &CANDIDATES[index];

    slot.zoom.store(zoom.to_bits(), Ordering::Relaxed);
    slot.center_x.store(center_x.to_bits(), Ordering::Relaxed);
    slot.center_y.store(center_y.to_bits(), Ordering::Relaxed);
    slot.extent_a.store(extent_a.to_bits(), Ordering::Relaxed);
    slot.extent_b.store(extent_b.to_bits(), Ordering::Relaxed);
    slot.mode.store(mode as u32, Ordering::Relaxed);
    slot.calls.fetch_add(1, Ordering::Release);
}

pub fn ensure_installed() -> Result<(), String> {
    INSTALL_RESULT
        .get_or_init(|| unsafe { install_inner() })
        .clone()
}

pub fn clear_candidates() {
    for slot in &CANDIDATES {
        slot.clear();
    }
}

pub fn snapshots() -> Vec<CameraSnapshot> {
    let mut out = Vec::new();

    for slot in &CANDIDATES {
        let address = slot.address.load(Ordering::Acquire);
        if address == 0 {
            continue;
        }

        out.push(CameraSnapshot {
            address,
            zoom: f32::from_bits(slot.zoom.load(Ordering::Relaxed)),
            center_x: f32::from_bits(slot.center_x.load(Ordering::Relaxed)),
            center_y: f32::from_bits(slot.center_y.load(Ordering::Relaxed)),
            extent_a: f32::from_bits(slot.extent_a.load(Ordering::Relaxed)),
            extent_b: f32::from_bits(slot.extent_b.load(Ordering::Relaxed)),
            mode: slot.mode.load(Ordering::Relaxed) as u8,
            calls: slot.calls.load(Ordering::Acquire),
        });
    }

    out.sort_by_key(|snapshot| snapshot.address);
    out
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

    let target = base.add(HANDLER_RVA);
    let actual = std::slice::from_raw_parts(target, PATCH_LEN);
    if actual != EXPECTED_PROLOGUE {
        return Err(format!(
            "camera handler signature mismatch at RVA 0x{HANDLER_RVA:X}"
        ));
    }

    let trampoline = VirtualAlloc(
        ptr::null_mut(),
        TRAMPOLINE_LEN,
        MEM_COMMIT | MEM_RESERVE,
        PAGE_EXECUTE_READWRITE,
    )
    .cast::<u8>();
    if trampoline.is_null() {
        return Err("VirtualAlloc failed while creating camera trampoline".to_owned());
    }

    ptr::copy_nonoverlapping(target, trampoline, PATCH_LEN);
    write_abs_jump(trampoline.add(PATCH_LEN), target.add(PATCH_LEN) as usize);

    // Publish the trampoline before the target can begin calling our hook.
    TRAMPOLINE.store(trampoline as usize, Ordering::Release);

    let mut old_protect = 0u32;
    if VirtualProtect(
        target.cast::<c_void>(),
        PATCH_LEN,
        PAGE_EXECUTE_READWRITE,
        &mut old_protect,
    ) == 0
    {
        TRAMPOLINE.store(0, Ordering::Release);
        return Err("VirtualProtect failed while enabling camera detour write".to_owned());
    }

    write_abs_jump(target, camera_handler_hook as usize);

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
