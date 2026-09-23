//! Version-checked camera-state capture for known Teamfight Manager 2 builds.
//!
//! The official stable mod API deliberately does not expose the spectator camera.
//! For each verified executable we therefore detour the game's camera
//! input/update handler and capture only a few verified fields from its `this`
//! pointer.
//!
//! Match-wide camera gestures publish requested native pan and zoom values into
//! atomics. The detoured camera handler applies those requests synchronously
//! immediately before TFM2's original handler runs. This avoids racing the game's
//! own camera/input code from a background thread while still leaving TFM2
//! authoritative for camera-center integration, bounds, follow state, minimap
//! relocation, and rendering.
//!
//! Keep every version-specific RVA/offset in this module. Higher-level camera/control
//! code should consume `CameraSnapshot` and never know TFM2's private object layout.

use std::{
    ffi::c_void,
    mem::size_of,
    ptr,
    sync::{
        atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering},
        Mutex, OnceLock,
    },
};

use windows_sys::Win32::System::Memory::{
    VirtualQuery, MEMORY_BASIC_INFORMATION, PAGE_GUARD, PAGE_NOACCESS,
};

#[derive(Debug, Clone, Copy)]
struct CameraLayout {
    pe_timestamp: u32,
    image_size: u32,
    handler_rva: usize,
    zoom_offset: usize,
    center_x_offset: usize,
    center_y_offset: usize,
    extent_a_offset: usize,
    extent_b_offset: usize,
    mode_offset: Option<usize>,
    /// Optional pointer field on the camera handler's `this` object that owns the
    /// native All/Blue/Red byte. None means the byte lives directly on `this`.
    vision_object_offset: Option<usize>,
    vision_mode_offset: Option<usize>,
    /// Native code may require this owner-relative usize field to be zero before
    /// changing vision. Preserve the game's own guard when known.
    vision_write_guard_offset: Option<usize>,
    pan_x_offset: usize,
    pan_y_offset: usize,
}

const BUILD_0_5_8: CameraLayout = CameraLayout {
    pe_timestamp: 0x6A97_8218,
    image_size: 0x04A1_D000,
    handler_rva: 0x009E_6750,
    zoom_offset: 0xE0,
    center_x_offset: 0xE4,
    center_y_offset: 0xE8,
    extent_a_offset: 0xEC,
    extent_b_offset: 0xF0,
    mode_offset: Some(0xF4),
    // Not yet reverse-engineered on 0.5.8; do not guess.
    vision_object_offset: None,
    vision_mode_offset: None,
    vision_write_guard_offset: None,
    pan_x_offset: 0x418,
    pan_y_offset: 0x41C,
};

const BUILD_0_6_0: CameraLayout = CameraLayout {
    pe_timestamp: 0x6AAA_07D1,
    image_size: 0x0522_8000,
    handler_rva: 0x009C_EBF0,
    zoom_offset: 0xE0,
    center_x_offset: 0xE4,
    center_y_offset: 0xE8,
    extent_a_offset: 0xEC,
    extent_b_offset: 0xF0,
    mode_offset: Some(0x100),
    // 0.6.0's historical probe treated +0x63 as direct camera state. It was never
    // promoted to a write path; keep that legacy read-only behavior unchanged.
    vision_object_offset: None,
    vision_mode_offset: Some(0x63),
    vision_write_guard_offset: None,
    pan_x_offset: 0x428,
    pan_y_offset: 0x42C,
};

const BUILD_0_6_1: CameraLayout = CameraLayout {
    pe_timestamp: 0x6AB1_D950,
    image_size: 0x0526_4000,
    handler_rva: 0x00C2_DBE0,
    zoom_offset: 0xE0,
    center_x_offset: 0xE4,
    center_y_offset: 0xE8,
    extent_a_offset: 0xEC,
    extent_b_offset: 0xF0,
    // The relocated handler retains five of six v0.6.0 camera signatures, but the
    // +0x100 mode access is absent. Keep it unknown rather than reading a guessed byte.
    mode_offset: None,
    // v0.6.1 static analysis found the handler itself loading [this+0x418] and
    // writing 0/1/2 to [owner+0x63] for All/Blue/Red. Physical testing had shown
    // this+0x63 stays 0, which is why the earlier direct-object probe failed.
    vision_object_offset: Some(0x418),
    vision_mode_offset: Some(0x63),
    vision_write_guard_offset: Some(0x10),
    pan_x_offset: 0x428,
    pan_y_offset: 0x42C,
};

fn known_layout(timestamp: u32, image_size: u32) -> Option<&'static CameraLayout> {
    [&BUILD_0_5_8, &BUILD_0_6_0, &BUILD_0_6_1]
        .into_iter()
        .find(|layout| layout.pe_timestamp == timestamp && layout.image_size == image_size)
}

const ZOOM_STEP: f32 = 0.25;
const ZOOM_MIN: f32 = 0.5;
const ZOOM_MAX: f32 = 3.0;

// The first 12 bytes of the confirmed handler are eight whole push instructions,
// so they can be copied to a trampoline without relocating RIP-relative code.
const PATCH_LEN: usize = 12;
const EXPECTED_PROLOGUE: [u8; PATCH_LEN] = [
    0x55, 0x41, 0x57, 0x41, 0x56, 0x41, 0x55, 0x41, 0x54, 0x56, 0x57, 0x53,
];
const ABS_JUMP_LEN: usize = 12;
const TRAMPOLINE_LEN: usize = PATCH_LEN + ABS_JUMP_LEN;
const MAX_CANDIDATES: usize = 4;

// Temporary native-follow differential probe. 0x430 is within the already-validated camera object
// span because v0.6.1's pan_y field lives at +0x42C.
const FOLLOW_PROBE_BYTES: usize = 0x430;
const FOLLOW_PROBE_WORDS: usize = FOLLOW_PROBE_BYTES / 8;
const VK_F1_CODE: i32 = 0x70;
const FOLLOW_PROBE_MAX_DIFFS: usize = 16;
const FOLLOW_ARG_OBJECT_BYTES: usize = 0x400;
const FOLLOW_ARG_OBJECT_WORDS: usize = FOLLOW_ARG_OBJECT_BYTES / 8;
const FOLLOW_POINTER_ARG_COUNT: usize = 5;
const FOLLOW_POINTER_ARG_INDEXES: [usize; FOLLOW_POINTER_ARG_COUNT] = [0, 1, 3, 4, 5];

const MEM_COMMIT: u32 = 0x1000;
const MEM_RESERVE: u32 = 0x2000;
const PAGE_EXECUTE_READWRITE: u32 = 0x40;

#[link(name = "user32")]
extern "system" {
    fn GetAsyncKeyState(vkey: i32) -> i16;
}

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
    pub mode: Option<u8>,
    pub vision_mode: Option<u8>,
    pub calls: u64,
}


#[derive(Debug, Clone, Copy)]
pub struct FollowProbeDiff {
    pub offset: usize,
    pub free: u64,
    pub held: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct FollowProbePointerDiff {
    pub arg: usize,
    pub offset: usize,
    pub free: u64,
    pub held: u64,
}

#[derive(Debug, Clone)]
pub struct FollowProbeReport {
    pub slot: usize,
    pub samples: u32,
    pub diffs: Vec<FollowProbeDiff>,
    pub arg_diffs: Vec<FollowProbeDiff>,
    pub pointer_diffs: Vec<FollowProbePointerDiff>,
}

struct FollowProbeState {
    last_mask: u16,
    free_valid: bool,
    free_words: [u64; FOLLOW_PROBE_WORDS],
    held_words: [u64; FOLLOW_PROBE_WORDS],
    stable: [bool; FOLLOW_PROBE_WORDS],
    held_samples: u32,
    held_slot: usize,
    report: Option<FollowProbeReport>,
    free_args: [u64; 6],
    held_args: [u64; 6],
    arg_stable: [bool; 6],
    free_ptrs: [usize; FOLLOW_POINTER_ARG_COUNT],
    free_ptr_valid: [bool; FOLLOW_POINTER_ARG_COUNT],
    held_ptrs: [usize; FOLLOW_POINTER_ARG_COUNT],
    free_ptr_words: [[u64; FOLLOW_ARG_OBJECT_WORDS]; FOLLOW_POINTER_ARG_COUNT],
    held_ptr_words: [[u64; FOLLOW_ARG_OBJECT_WORDS]; FOLLOW_POINTER_ARG_COUNT],
    ptr_stable: [[bool; FOLLOW_ARG_OBJECT_WORDS]; FOLLOW_POINTER_ARG_COUNT],
    ptr_valid: [bool; FOLLOW_POINTER_ARG_COUNT],
}

impl Default for FollowProbeState {
    fn default() -> Self {
        Self {
            last_mask: 0,
            free_valid: false,
            free_words: [0; FOLLOW_PROBE_WORDS],
            held_words: [0; FOLLOW_PROBE_WORDS],
            stable: [false; FOLLOW_PROBE_WORDS],
            held_samples: 0,
            held_slot: 0,
            report: None,
            free_args: [0; 6],
            held_args: [0; 6],
            arg_stable: [false; 6],
            free_ptrs: [0; FOLLOW_POINTER_ARG_COUNT],
            free_ptr_valid: [false; FOLLOW_POINTER_ARG_COUNT],
            held_ptrs: [0; FOLLOW_POINTER_ARG_COUNT],
            free_ptr_words: [[0; FOLLOW_ARG_OBJECT_WORDS]; FOLLOW_POINTER_ARG_COUNT],
            held_ptr_words: [[0; FOLLOW_ARG_OBJECT_WORDS]; FOLLOW_POINTER_ARG_COUNT],
            ptr_stable: [[false; FOLLOW_ARG_OBJECT_WORDS]; FOLLOW_POINTER_ARG_COUNT],
            ptr_valid: [false; FOLLOW_POINTER_ARG_COUNT],
        }
    }
}

struct CandidateSlot {
    address: AtomicUsize,
    zoom: AtomicU32,
    center_x: AtomicU32,
    center_y: AtomicU32,
    extent_a: AtomicU32,
    extent_b: AtomicU32,
    mode: AtomicU32,
    vision_mode: AtomicU32,
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
            mode: AtomicU32::new(u32::MAX),
            vision_mode: AtomicU32::new(u32::MAX),
            calls: AtomicU64::new(0),
        }
    }

    fn clear(&self) {
        self.calls.store(0, Ordering::Relaxed);
        self.mode.store(u32::MAX, Ordering::Relaxed);
        self.vision_mode.store(u32::MAX, Ordering::Relaxed);
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
static ACTIVE_LAYOUT: OnceLock<&'static CameraLayout> = OnceLock::new();
static TRAMPOLINE: AtomicUsize = AtomicUsize::new(0);
static INSTALL_RESULT: OnceLock<Result<(), String>> = OnceLock::new();
static FOLLOW_PROBE_STATE: OnceLock<Mutex<FollowProbeState>> = OnceLock::new();

// Requested pan is published by the match-wide camera driver. The actual private
// camera fields are touched only from inside the native camera-handler thread.
static REQUESTED_PAN_ADDRESS: AtomicUsize = AtomicUsize::new(0);
static REQUESTED_PAN_X: AtomicU32 = AtomicU32::new(0);
static REQUESTED_PAN_Y: AtomicU32 = AtomicU32::new(0);
static REQUESTED_PAN_ACTIVE: AtomicBool = AtomicBool::new(false);
static REQUESTED_PAN_CLEAR_ONCE: AtomicBool = AtomicBool::new(false);

// Mouse-wheel input is captured at the match window, but the version-specific zoom
// field is only changed here, synchronously on the camera-handler thread. Multiple
// notches may accumulate before the next matching camera callback.
static REQUESTED_ZOOM_ADDRESS: AtomicUsize = AtomicUsize::new(0);
static REQUESTED_ZOOM_STEPS: AtomicI32 = AtomicI32::new(0);

// Automatic team fog is published from the render/control layer but applied only
// on the already-detoured native camera thread. u32::MAX means "do not enforce".
static REQUESTED_VISION_MODE: AtomicU32 = AtomicU32::new(u32::MAX);

// Observed machine-level signature at the two known call sites. Only the first
// argument matters to us; preserving the rest exactly lets the original handler
// continue normally through the trampoline.
type CameraHandlerFn = unsafe extern "system" fn(*mut u8, usize, usize, f32, usize, usize, usize);

unsafe extern "system" fn camera_handler_hook(
    this: *mut u8,
    arg2: usize,
    arg3: usize,
    arg4: f32,
    arg5: usize,
    arg6: usize,
    arg7: usize,
) {
    inject_requested_zoom(this);
    inject_requested_pan(this);

    let trampoline = TRAMPOLINE.load(Ordering::Acquire);
    if trampoline != 0 {
        let original: CameraHandlerFn = std::mem::transmute(trampoline);
        original(this, arg2, arg3, arg4, arg5, arg6, arg7);
    }

    // Apply team fog after native input handling so an ordinary spectator view-button
    // click cannot override the controlled champion's required team vision mid-frame.
    inject_requested_vision(this);
    capture(this);
    update_follow_probe(
        this,
        [
            arg2 as u64,
            arg3 as u64,
            arg4.to_bits() as u64,
            arg5 as u64,
            arg6 as u64,
            arg7 as u64,
        ],
    );
}

unsafe fn inject_requested_zoom(this: *mut u8) {
    if this.is_null() {
        return;
    }

    let address = this as usize;
    if REQUESTED_ZOOM_ADDRESS.load(Ordering::Acquire) != address {
        return;
    }

    let steps = REQUESTED_ZOOM_STEPS.swap(0, Ordering::AcqRel);
    if steps == 0 {
        return;
    }

    let Some(layout) = ACTIVE_LAYOUT.get().copied() else {
        return;
    };
    let zoom = ptr::read_unaligned(this.add(layout.zoom_offset).cast::<f32>());
    if !zoom.is_finite() {
        return;
    }

    // Static and runtime RE confirmed native zoom moves in 0.25 increments and clamps
    // to 0.5..3.0. Writing the requested zoom before the original handler lets TFM2's
    // own camera update continue to own its derived extents and render state.
    let next = (zoom + steps as f32 * ZOOM_STEP).clamp(ZOOM_MIN, ZOOM_MAX);
    ptr::write_unaligned(this.add(layout.zoom_offset).cast::<f32>(), next);
}

unsafe fn inject_requested_pan(this: *mut u8) {
    if this.is_null() {
        return;
    }

    let address = this as usize;
    if REQUESTED_PAN_ADDRESS.load(Ordering::Acquire) != address {
        return;
    }

    let Some(layout) = ACTIVE_LAYOUT.get().copied() else {
        return;
    };

    if REQUESTED_PAN_ACTIVE.load(Ordering::Acquire) {
        let pan_x = f32::from_bits(REQUESTED_PAN_X.load(Ordering::Relaxed));
        let pan_y = f32::from_bits(REQUESTED_PAN_Y.load(Ordering::Relaxed));
        if pan_x.is_finite() && pan_y.is_finite() {
            ptr::write_unaligned(this.add(layout.pan_x_offset).cast::<f32>(), pan_x);
            ptr::write_unaligned(this.add(layout.pan_y_offset).cast::<f32>(), pan_y);
        }
    } else if REQUESTED_PAN_CLEAR_ONCE.swap(false, Ordering::AcqRel) {
        // Clear one final injected value when the gesture ends, then stop touching
        // native pan state so TFM2's own controls regain full ownership.
        ptr::write_unaligned(this.add(layout.pan_x_offset).cast::<f32>(), 0.0);
        ptr::write_unaligned(this.add(layout.pan_y_offset).cast::<f32>(), 0.0);
    }
}

unsafe fn vision_object(this: *mut u8, layout: &CameraLayout) -> Option<*mut u8> {
    let object = match layout.vision_object_offset {
        Some(offset) => ptr::read_unaligned(this.add(offset).cast::<*mut u8>()),
        None => this,
    };
    (!object.is_null()).then_some(object)
}

unsafe fn inject_requested_vision(this: *mut u8) {
    if this.is_null() {
        return;
    }

    let requested = REQUESTED_VISION_MODE.load(Ordering::Acquire);
    if requested > 2 {
        return;
    }

    let Some(layout) = ACTIVE_LAYOUT.get().copied() else {
        return;
    };
    let Some(mode_offset) = layout.vision_mode_offset else {
        return;
    };
    let Some(object) = vision_object(this, layout) else {
        return;
    };

    if let Some(guard_offset) = layout.vision_write_guard_offset {
        if ptr::read_unaligned(object.add(guard_offset).cast::<usize>()) != 0 {
            return;
        }
    }

    ptr::write_unaligned(object.add(mode_offset).cast::<u8>(), requested as u8);
}

pub fn set_vision_mode(mode: Option<u8>) {
    REQUESTED_VISION_MODE.store(
        mode.filter(|value| *value <= 2)
            .map(u32::from)
            .unwrap_or(u32::MAX),
        Ordering::Release,
    );
}

pub fn queue_zoom_steps(address: usize, steps: i32) {
    if address == 0 || steps == 0 {
        return;
    }

    REQUESTED_ZOOM_ADDRESS.store(address, Ordering::Release);
    REQUESTED_ZOOM_STEPS.fetch_add(steps, Ordering::AcqRel);
}

pub fn set_pan_request(address: usize, pan_x: f32, pan_y: f32) {
    if address == 0 || !pan_x.is_finite() || !pan_y.is_finite() {
        return;
    }

    REQUESTED_PAN_X.store(pan_x.to_bits(), Ordering::Relaxed);
    REQUESTED_PAN_Y.store(pan_y.to_bits(), Ordering::Relaxed);
    REQUESTED_PAN_ADDRESS.store(address, Ordering::Release);
    REQUESTED_PAN_CLEAR_ONCE.store(false, Ordering::Release);
    REQUESTED_PAN_ACTIVE.store(true, Ordering::Release);
}

pub fn clear_pan_request(address: usize) {
    if address == 0 {
        REQUESTED_PAN_ACTIVE.store(false, Ordering::Release);
        REQUESTED_PAN_CLEAR_ONCE.store(false, Ordering::Release);
        REQUESTED_PAN_ADDRESS.store(0, Ordering::Release);
        return;
    }

    REQUESTED_PAN_X.store(0.0f32.to_bits(), Ordering::Relaxed);
    REQUESTED_PAN_Y.store(0.0f32.to_bits(), Ordering::Relaxed);
    REQUESTED_PAN_ADDRESS.store(address, Ordering::Release);
    REQUESTED_PAN_ACTIVE.store(false, Ordering::Release);
    REQUESTED_PAN_CLEAR_ONCE.store(true, Ordering::Release);
}

fn follow_probe_state() -> &'static Mutex<FollowProbeState> {
    FOLLOW_PROBE_STATE.get_or_init(|| Mutex::new(FollowProbeState::default()))
}

unsafe fn fkey_mask() -> u16 {
    let mut mask = 0u16;
    for slot in 0..10usize {
        if GetAsyncKeyState(VK_F1_CODE + slot as i32) < 0 {
            mask |= 1u16 << slot;
        }
    }
    mask
}

unsafe fn readable_qwords(address: usize) -> Option<[u64; FOLLOW_ARG_OBJECT_WORDS]> {
    if address < 0x1_0000 {
        return None;
    }

    let mut info = std::mem::zeroed::<MEMORY_BASIC_INFORMATION>();
    if VirtualQuery(
        address as *const c_void,
        &mut info,
        size_of::<MEMORY_BASIC_INFORMATION>(),
    ) == 0
    {
        return None;
    }
    if info.State != MEM_COMMIT
        || info.Protect & PAGE_GUARD != 0
        || info.Protect & PAGE_NOACCESS != 0
    {
        return None;
    }

    let region_start = info.BaseAddress as usize;
    let region_end = region_start.checked_add(info.RegionSize)?;
    let read_end = address.checked_add(FOLLOW_ARG_OBJECT_BYTES)?;
    if address < region_start || read_end > region_end {
        return None;
    }

    let mut words = [0u64; FOLLOW_ARG_OBJECT_WORDS];
    for (index, word) in words.iter_mut().enumerate() {
        *word = ptr::read_unaligned((address + index * 8) as *const u64);
    }
    Some(words)
}

unsafe fn read_pointer_probe_args(
    args: [u64; 6],
) -> (
    [usize; FOLLOW_POINTER_ARG_COUNT],
    [Option<[u64; FOLLOW_ARG_OBJECT_WORDS]>; FOLLOW_POINTER_ARG_COUNT],
) {
    let mut ptrs = [0usize; FOLLOW_POINTER_ARG_COUNT];
    let mut words: [Option<[u64; FOLLOW_ARG_OBJECT_WORDS]>; FOLLOW_POINTER_ARG_COUNT] =
        std::array::from_fn(|_| None);

    for (probe_index, arg_index) in FOLLOW_POINTER_ARG_INDEXES.iter().copied().enumerate() {
        let address = args[arg_index] as usize;
        ptrs[probe_index] = address;
        words[probe_index] = readable_qwords(address);
    }
    (ptrs, words)
}

unsafe fn read_follow_probe_words(this: *mut u8) -> [u64; FOLLOW_PROBE_WORDS] {
    let mut words = [0u64; FOLLOW_PROBE_WORDS];
    for (index, word) in words.iter_mut().enumerate() {
        *word = ptr::read_unaligned(this.add(index * 8).cast::<u64>());
    }
    words
}

unsafe fn update_follow_probe(this: *mut u8, args: [u64; 6]) {
    let mask = fkey_mask();
    let words = read_follow_probe_words(this);
    let (pointer_args, pointer_words) = read_pointer_probe_args(args);
    let Ok(mut state) = follow_probe_state().lock() else {
        return;
    };

    if mask == 0 {
        if state.last_mask != 0 && state.free_valid && state.held_samples >= 3 {
            let mut diffs = Vec::new();
            for index in 0..FOLLOW_PROBE_WORDS {
                if !state.stable[index] || state.free_words[index] == state.held_words[index] {
                    continue;
                }

                // Known continuously changing camera geometry/input fields are not useful for
                // identifying native follow ownership.
                let offset = index * 8;
                if (0xE0..=0xF0).contains(&offset)
                    || (0x428..=0x42C).contains(&offset)
                {
                    continue;
                }

                diffs.push(FollowProbeDiff {
                    offset,
                    free: state.free_words[index],
                    held: state.held_words[index],
                });
                if diffs.len() >= FOLLOW_PROBE_MAX_DIFFS {
                    break;
                }
            }

            let mut arg_diffs = Vec::new();
            for index in 0..6 {
                if state.arg_stable[index] && state.free_args[index] != state.held_args[index] {
                    arg_diffs.push(FollowProbeDiff {
                        // 0xF00+N is a HUD-only namespace for handler arguments, not object memory.
                        offset: 0xF00 + index,
                        free: state.free_args[index],
                        held: state.held_args[index],
                    });
                }
            }

            let mut pointer_diffs = Vec::new();
            for probe_index in 0..FOLLOW_POINTER_ARG_COUNT {
                if !state.ptr_valid[probe_index]
                    || state.free_ptrs[probe_index] == 0
                    || state.free_ptrs[probe_index] != state.held_ptrs[probe_index]
                {
                    continue;
                }

                let arg_index = FOLLOW_POINTER_ARG_INDEXES[probe_index];
                for word_index in 0..FOLLOW_ARG_OBJECT_WORDS {
                    if !state.ptr_stable[probe_index][word_index]
                        || state.free_ptr_words[probe_index][word_index]
                            == state.held_ptr_words[probe_index][word_index]
                    {
                        continue;
                    }

                    pointer_diffs.push(FollowProbePointerDiff {
                        arg: arg_index + 2,
                        offset: word_index * 8,
                        free: state.free_ptr_words[probe_index][word_index],
                        held: state.held_ptr_words[probe_index][word_index],
                    });
                    if pointer_diffs.len() >= FOLLOW_PROBE_MAX_DIFFS {
                        break;
                    }
                }
                if pointer_diffs.len() >= FOLLOW_PROBE_MAX_DIFFS {
                    break;
                }
            }

            state.report = Some(FollowProbeReport {
                slot: state.held_slot,
                samples: state.held_samples,
                diffs,
                arg_diffs,
                pointer_diffs,
            });
        }

        // Keep the most recent genuinely free-camera frame as the baseline. The report is retained
        // until the next completed F-key hold so the render HUD can show it after release.
        state.free_words = words;
        state.free_args = args;
        state.free_valid = true;
        for probe_index in 0..FOLLOW_POINTER_ARG_COUNT {
            state.free_ptrs[probe_index] = pointer_args[probe_index];
            state.free_ptr_valid[probe_index] = false;
            if let Some(snapshot) = pointer_words[probe_index] {
                state.free_ptr_words[probe_index] = snapshot;
                state.free_ptr_valid[probe_index] = true;
            }
        }
        state.last_mask = 0;
        state.held_samples = 0;
        return;
    }

    let slot = mask.trailing_zeros() as usize;
    if state.last_mask == 0 || state.last_mask != mask {
        state.held_words = words;
        state.stable.fill(true);
        state.held_args = args;
        state.arg_stable.fill(true);
        state.held_samples = 1;
        for probe_index in 0..FOLLOW_POINTER_ARG_COUNT {
            state.held_ptrs[probe_index] = pointer_args[probe_index];
            state.ptr_stable[probe_index].fill(true);
            state.ptr_valid[probe_index] = false;

            if state.free_ptr_valid[probe_index]
                && pointer_args[probe_index] != 0
                && pointer_args[probe_index] == state.free_ptrs[probe_index]
            {
                if let Some(snapshot) = pointer_words[probe_index] {
                    state.held_ptr_words[probe_index] = snapshot;
                    state.ptr_valid[probe_index] = true;
                }
            }
        }
        state.held_slot = slot;
    } else {
        // Give native follow a few camera callbacks to settle before judging stability. During this
        // window we simply move the held baseline forward; from the fourth held callback onward,
        // any further mutation marks that field as dynamic rather than follow ownership/state.
        let settling = state.held_samples < 3;

        if !settling {
            for (index, word) in words.iter().enumerate() {
                if state.held_words[index] != *word {
                    state.stable[index] = false;
                }
            }
            for index in 0..6 {
                if state.held_args[index] != args[index] {
                    state.arg_stable[index] = false;
                }
            }
        }
        state.held_words = words;
        state.held_args = args;

        for probe_index in 0..FOLLOW_POINTER_ARG_COUNT {
            if !state.ptr_valid[probe_index]
                || pointer_args[probe_index] != state.held_ptrs[probe_index]
            {
                state.ptr_valid[probe_index] = false;
                continue;
            }
            let Some(snapshot) = pointer_words[probe_index] else {
                state.ptr_valid[probe_index] = false;
                continue;
            };
            if !settling {
                for word_index in 0..FOLLOW_ARG_OBJECT_WORDS {
                    if state.held_ptr_words[probe_index][word_index] != snapshot[word_index] {
                        state.ptr_stable[probe_index][word_index] = false;
                    }
                }
            }
            state.held_ptr_words[probe_index] = snapshot;
        }

        state.held_samples = state.held_samples.saturating_add(1);
    }
    state.last_mask = mask;
}

pub fn follow_probe_report() -> Option<FollowProbeReport> {
    follow_probe_state()
        .lock()
        .ok()
        .and_then(|state| state.report.clone())
}

unsafe fn capture(this: *mut u8) {
    if this.is_null() {
        return;
    }

    let Some(layout) = ACTIVE_LAYOUT.get().copied() else {
        return;
    };
    let zoom = ptr::read_unaligned(this.add(layout.zoom_offset).cast::<f32>());
    let center_x = ptr::read_unaligned(this.add(layout.center_x_offset).cast::<f32>());
    let center_y = ptr::read_unaligned(this.add(layout.center_y_offset).cast::<f32>());
    let extent_a = ptr::read_unaligned(this.add(layout.extent_a_offset).cast::<f32>());
    let extent_b = ptr::read_unaligned(this.add(layout.extent_b_offset).cast::<f32>());
    let mode = layout
        .mode_offset
        .map(|offset| ptr::read_unaligned(this.add(offset).cast::<u8>()));
    let vision_mode = match layout.vision_mode_offset {
        Some(offset) => vision_object(this, layout)
            .map(|object| ptr::read_unaligned(object.add(offset).cast::<u8>())),
        None => None,
    };

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
    slot.mode.store(
        mode.map(u32::from).unwrap_or(u32::MAX),
        Ordering::Relaxed,
    );
    slot.vision_mode.store(
        vision_mode.map(u32::from).unwrap_or(u32::MAX),
        Ordering::Relaxed,
    );
    slot.calls.fetch_add(1, Ordering::Release);
}

pub fn ensure_installed() -> Result<(), String> {
    INSTALL_RESULT
        .get_or_init(|| unsafe { install_inner() })
        .clone()
}

pub fn clear_follow_probe() {
    if let Ok(mut state) = follow_probe_state().lock() {
        *state = FollowProbeState::default();
    }
}

pub fn clear_candidates() {
    REQUESTED_PAN_ACTIVE.store(false, Ordering::Release);
    REQUESTED_PAN_CLEAR_ONCE.store(false, Ordering::Release);
    REQUESTED_PAN_ADDRESS.store(0, Ordering::Release);
    REQUESTED_ZOOM_STEPS.store(0, Ordering::Release);
    REQUESTED_ZOOM_ADDRESS.store(0, Ordering::Release);
    REQUESTED_VISION_MODE.store(u32::MAX, Ordering::Release);
    clear_follow_probe();

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
            mode: match slot.mode.load(Ordering::Relaxed) {
                u32::MAX => None,
                value => Some(value as u8),
            },
            vision_mode: match slot.vision_mode.load(Ordering::Relaxed) {
                u32::MAX => None,
                value => Some(value as u8),
            },
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
    let layout = known_layout(timestamp, image_size).ok_or_else(|| {
        format!(
            "unsupported TeamfightManager2.exe build (timestamp=0x{timestamp:08X}, image=0x{image_size:08X})"
        )
    })?;
    let _ = ACTIVE_LAYOUT.set(layout);

    let target = base.add(layout.handler_rva);
    let actual = std::slice::from_raw_parts(target, PATCH_LEN);
    if actual != EXPECTED_PROLOGUE {
        return Err(format!(
            "camera handler signature mismatch at RVA 0x{:X}",
            layout.handler_rva
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
