//! Version-checked camera-state capture for Teamfight Manager 2 v0.5.8.
//!
//! The official stable mod API deliberately does not expose the spectator camera.
//! For the single tested 0.5.8 executable we therefore detour the game's camera
//! input/update handler and capture only a few verified fields from its `this`
//! pointer.
//!
//! Camera gestures are sampled from the StableClient render loop, not from the
//! native camera-input handler. The native handler is input/event-cadenced: physical
//! testing showed stationary edge-scroll only advanced in small ticks and fast MMB
//! drags skipped between sparse samples. The render loop gives us one deterministic
//! camera update per presented frame.
//!
//! MMB is an anchored grab: while held, camera center is derived from the total
//! logical-UI cursor displacement from the original press point. Edge scrolling uses
//! the live `ingame.center_log` battlefield rectangle and integrates smoothly by
//! wall-clock frame delta. A tiny native pan pulse is used only to tell TFM2 that a
//! manual camera gesture occurred so its follow/Auto Camera state can disengage; the
//! native pan velocity never determines gesture distance.
//!
//! Keep every version-specific RVA/offset in this module. Higher-level direct
//! control code should consume `CameraSnapshot` and never know TFM2's private
//! object layout.

use std::{
    ffi::c_void,
    ptr,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering},
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
const PAN_X_OFFSET: usize = 0x418;
const PAN_Y_OFFSET: usize = 0x41C;

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

const GAME_DRAW_SIZE: f32 = 2048.0;
const WORLD_MIN: f32 = 0.0;
const WORLD_MAX: f32 = 960.0;
const EDGE_SCROLL_MARGIN_UI: f32 = 18.0;
const EDGE_SCROLL_VIEWPORTS_PER_SECOND: f32 = 1.35;
const DEFAULT_FRAME_DT_SECONDS: f32 = 1.0 / 60.0;
const MAX_FRAME_DT_SECONDS: f32 = 0.050;
const NATIVE_MANUAL_INTENT_SPEED: f32 = 1.0;
const VK_MBUTTON_CODE: i32 = 0x04;
const NO_ATHLETE: usize = usize::MAX;

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(module_name: *const u16) -> *mut c_void;
    fn GetCurrentProcess() -> *mut c_void;
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

#[link(name = "user32")]
extern "system" {
    fn GetAsyncKeyState(vkey: i32) -> i16;
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

static LAST_SELECTED_ATHLETE: AtomicUsize = AtomicUsize::new(NO_ATHLETE);
static NATIVE_INTENT_PENDING: AtomicBool = AtomicBool::new(false);
static MMB_WAS_DOWN: AtomicBool = AtomicBool::new(false);
static DRAG_MOVED: AtomicBool = AtomicBool::new(false);
static DRAG_START_MOUSE_X: AtomicU32 = AtomicU32::new(0);
static DRAG_START_MOUSE_Y: AtomicU32 = AtomicU32::new(0);
static DRAG_START_CENTER_X: AtomicU32 = AtomicU32::new(0);
static DRAG_START_CENTER_Y: AtomicU32 = AtomicU32::new(0);
static DRAG_START_EXTENT_X: AtomicU32 = AtomicU32::new(0);
static DRAG_START_EXTENT_Y: AtomicU32 = AtomicU32::new(0);
static EDGE_WAS_ACTIVE: AtomicBool = AtomicBool::new(false);
static EDGE_CENTER_X: AtomicU32 = AtomicU32::new(0);
static EDGE_CENTER_Y: AtomicU32 = AtomicU32::new(0);
static LAST_FRAME_MS: AtomicU64 = AtomicU64::new(0);

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

    // Native pan is only a semantic pulse to make TFM2 observe "manual camera
    // movement" and leave follow/Auto Camera. Clear it immediately after the base
    // handler consumes it so it can never become our actual camera velocity.
    if NATIVE_INTENT_PENDING.swap(false, Ordering::AcqRel) {
        ptr::write_unaligned(this.add(PAN_X_OFFSET).cast::<f32>(), 0.0);
        ptr::write_unaligned(this.add(PAN_Y_OFFSET).cast::<f32>(), 0.0);
    }

    capture(this);
}

fn direct_control_camera_active() -> bool {
    let selected = crate::control::selected_athlete().unwrap_or(NO_ATHLETE);
    let previous = LAST_SELECTED_ATHLETE.swap(selected, Ordering::AcqRel);
    if selected != previous {
        reset_manual_camera_state();
    }

    selected != NO_ATHLETE && crate::pacing_probe::manual_input_enabled()
}

fn reset_manual_camera_state() {
    MMB_WAS_DOWN.store(false, Ordering::Release);
    DRAG_MOVED.store(false, Ordering::Release);
    EDGE_WAS_ACTIVE.store(false, Ordering::Release);
    LAST_FRAME_MS.store(0, Ordering::Release);
}

fn preferred_snapshot() -> Option<CameraSnapshot> {
    let mut best: Option<CameraSnapshot> = None;

    for slot in &CANDIDATES {
        let address = slot.address.load(Ordering::Acquire);
        if address == 0 {
            continue;
        }
        let snapshot = CameraSnapshot {
            address,
            zoom: f32::from_bits(slot.zoom.load(Ordering::Relaxed)),
            center_x: f32::from_bits(slot.center_x.load(Ordering::Relaxed)),
            center_y: f32::from_bits(slot.center_y.load(Ordering::Relaxed)),
            extent_a: f32::from_bits(slot.extent_a.load(Ordering::Relaxed)),
            extent_b: f32::from_bits(slot.extent_b.load(Ordering::Relaxed)),
            mode: slot.mode.load(Ordering::Relaxed) as u8,
            calls: slot.calls.load(Ordering::Acquire),
        };
        if best.map_or(true, |current| snapshot.calls > current.calls) {
            best = Some(snapshot);
        }
    }

    best
}

fn frame_dt_seconds() -> f32 {
    let now = unsafe { GetTickCount64() };
    let previous = LAST_FRAME_MS.swap(now, Ordering::AcqRel);
    if previous == 0 {
        return DEFAULT_FRAME_DT_SECONDS;
    }
    (now.saturating_sub(previous) as f32 / 1000.0).clamp(0.0, MAX_FRAME_DT_SECONDS)
}

fn clamp_center(center: f32, extent: f32) -> f32 {
    let half = (extent.abs() * 0.5).clamp(0.0, (WORLD_MAX - WORLD_MIN) * 0.5);
    center.clamp(WORLD_MIN + half, WORLD_MAX - half)
}

fn edge_axis(
    mouse_x: f32,
    mouse_y: f32,
    viewport: (f32, f32, f32, f32),
) -> (f32, f32) {
    let (x, y, w, h) = viewport;
    if w <= 0.0 || h <= 0.0 {
        return (0.0, 0.0);
    }
    let right = x + w;
    let bottom = y + h;

    // Horizontal scrolling belongs to the battlefield boundary itself. This keeps
    // the right-hand information panel from becoming an enormous invisible edge in
    // split/Match-Info mode.
    let in_vertical_span = mouse_y >= y && mouse_y <= bottom;
    let left = in_vertical_span
        && mouse_x >= x - EDGE_SCROLL_MARGIN_UI
        && mouse_x <= x + EDGE_SCROLL_MARGIN_UI;
    let right_edge = in_vertical_span
        && mouse_x >= right - EDGE_SCROLL_MARGIN_UI
        && mouse_x <= right + EDGE_SCROLL_MARGIN_UI;

    // TFM2's top team-stat bar and bottom controls sit outside center_log. Treat
    // those bands as extensions of the nearest vertical battlefield edge so moving
    // the cursor all the way to the physical top/bottom still scrolls instead of
    // entering a dead UI zone.
    let in_horizontal_span = mouse_x >= x && mouse_x <= right;
    let up = in_horizontal_span && mouse_y <= y + EDGE_SCROLL_MARGIN_UI;
    let down = in_horizontal_span && mouse_y >= bottom - EDGE_SCROLL_MARGIN_UI;

    let mut axis_x = (right_edge as i32 - left as i32) as f32;
    let mut axis_y = (down as i32 - up as i32) as f32;
    if axis_x != 0.0 && axis_y != 0.0 {
        const INV_SQRT_2: f32 = 0.707_106_77;
        axis_x *= INV_SQRT_2;
        axis_y *= INV_SQRT_2;
    }
    (axis_x, axis_y)
}

unsafe fn issue_native_manual_intent(address: usize, axis_x: f32, axis_y: f32) {
    if address == 0 || (axis_x == 0.0 && axis_y == 0.0) {
        return;
    }
    let this = address as *mut u8;
    ptr::write_unaligned(
        this.add(PAN_X_OFFSET).cast::<f32>(),
        axis_x.signum() * NATIVE_MANUAL_INTENT_SPEED,
    );
    ptr::write_unaligned(
        this.add(PAN_Y_OFFSET).cast::<f32>(),
        axis_y.signum() * NATIVE_MANUAL_INTENT_SPEED,
    );
    NATIVE_INTENT_PENDING.store(true, Ordering::Release);
}

unsafe fn clear_stale_native_intent(address: usize) {
    if address == 0 || !NATIVE_INTENT_PENDING.swap(false, Ordering::AcqRel) {
        return;
    }
    let this = address as *mut u8;
    ptr::write_unaligned(this.add(PAN_X_OFFSET).cast::<f32>(), 0.0);
    ptr::write_unaligned(this.add(PAN_Y_OFFSET).cast::<f32>(), 0.0);
}

unsafe fn write_camera_center(snapshot: CameraSnapshot, center_x: f32, center_y: f32) {
    if snapshot.address == 0 || !center_x.is_finite() || !center_y.is_finite() {
        return;
    }

    let this = snapshot.address as *mut u8;
    ptr::write_unaligned(this.add(CENTER_X_OFFSET).cast::<f32>(), center_x);
    ptr::write_unaligned(this.add(CENTER_Y_OFFSET).cast::<f32>(), center_y);

    for slot in &CANDIDATES {
        if slot.address.load(Ordering::Acquire) == snapshot.address {
            slot.center_x.store(center_x.to_bits(), Ordering::Release);
            slot.center_y.store(center_y.to_bits(), Ordering::Release);
            break;
        }
    }
}

/// Update Direct Control camera gestures once per presented frame.
///
/// `mouse_x` / `mouse_y` and `battlefield` are in the stable UI logical coordinate
/// space. This intentionally runs from `post_render`; the native camera-input hook is
/// not a reliable frame clock and was physically observed to tick only when input
/// events arrived.
pub fn update_frame_controls(
    mouse_valid: bool,
    mouse_x: f32,
    mouse_y: f32,
    battlefield: Option<(f32, f32, f32, f32)>,
) {
    let Some(snapshot) = preferred_snapshot() else {
        reset_manual_camera_state();
        return;
    };

    if !direct_control_camera_active() || !mouse_valid {
        unsafe { clear_stale_native_intent(snapshot.address) };
        reset_manual_camera_state();
        return;
    }

    let Some(viewport) = battlefield else {
        unsafe { clear_stale_native_intent(snapshot.address) };
        reset_manual_camera_state();
        return;
    };

    if !snapshot.center_x.is_finite()
        || !snapshot.center_y.is_finite()
        || !snapshot.extent_a.is_finite()
        || !snapshot.extent_b.is_finite()
        || snapshot.extent_a <= 0.0
        || snapshot.extent_b <= 0.0
    {
        reset_manual_camera_state();
        return;
    }

    let dt = frame_dt_seconds();
    let middle_down = unsafe { GetAsyncKeyState(VK_MBUTTON_CODE) < 0 };

    if middle_down {
        EDGE_WAS_ACTIVE.store(false, Ordering::Release);
        let was_down = MMB_WAS_DOWN.swap(true, Ordering::AcqRel);
        if !was_down {
            DRAG_START_MOUSE_X.store(mouse_x.to_bits(), Ordering::Release);
            DRAG_START_MOUSE_Y.store(mouse_y.to_bits(), Ordering::Release);
            DRAG_START_CENTER_X.store(snapshot.center_x.to_bits(), Ordering::Release);
            DRAG_START_CENTER_Y.store(snapshot.center_y.to_bits(), Ordering::Release);
            DRAG_START_EXTENT_X.store(snapshot.extent_a.to_bits(), Ordering::Release);
            DRAG_START_EXTENT_Y.store(snapshot.extent_b.to_bits(), Ordering::Release);
            DRAG_MOVED.store(false, Ordering::Release);
            return;
        }

        let start_mouse_x = f32::from_bits(DRAG_START_MOUSE_X.load(Ordering::Acquire));
        let start_mouse_y = f32::from_bits(DRAG_START_MOUSE_Y.load(Ordering::Acquire));
        let start_center_x = f32::from_bits(DRAG_START_CENTER_X.load(Ordering::Acquire));
        let start_center_y = f32::from_bits(DRAG_START_CENTER_Y.load(Ordering::Acquire));
        let start_extent_x = f32::from_bits(DRAG_START_EXTENT_X.load(Ordering::Acquire));
        let start_extent_y = f32::from_bits(DRAG_START_EXTENT_Y.load(Ordering::Acquire));

        let dx = mouse_x - start_mouse_x;
        let dy = mouse_y - start_mouse_y;
        if !dx.is_finite() || !dy.is_finite() {
            return;
        }

        if !DRAG_MOVED.load(Ordering::Acquire) && (dx.abs() >= 0.5 || dy.abs() >= 0.5) {
            unsafe { issue_native_manual_intent(snapshot.address, -dx, -dy) };
            DRAG_MOVED.store(true, Ordering::Release);
        }

        let center_x = clamp_center(
            start_center_x - dx * start_extent_x / GAME_DRAW_SIZE,
            start_extent_x,
        );
        let center_y = clamp_center(
            start_center_y - dy * start_extent_y / GAME_DRAW_SIZE,
            start_extent_y,
        );
        unsafe { write_camera_center(snapshot, center_x, center_y) };
        return;
    }

    if MMB_WAS_DOWN.swap(false, Ordering::AcqRel) {
        DRAG_MOVED.store(false, Ordering::Release);
    }

    let (axis_x, axis_y) = edge_axis(mouse_x, mouse_y, viewport);
    let edge_active = axis_x != 0.0 || axis_y != 0.0;
    let was_edge_active = EDGE_WAS_ACTIVE.swap(edge_active, Ordering::AcqRel);

    if !edge_active {
        unsafe { clear_stale_native_intent(snapshot.address) };
        return;
    }

    if !was_edge_active {
        EDGE_CENTER_X.store(snapshot.center_x.to_bits(), Ordering::Release);
        EDGE_CENTER_Y.store(snapshot.center_y.to_bits(), Ordering::Release);
        unsafe { issue_native_manual_intent(snapshot.address, axis_x, axis_y) };
    }

    let mut center_x = f32::from_bits(EDGE_CENTER_X.load(Ordering::Acquire));
    let mut center_y = f32::from_bits(EDGE_CENTER_Y.load(Ordering::Acquire));
    if !center_x.is_finite() || !center_y.is_finite() {
        center_x = snapshot.center_x;
        center_y = snapshot.center_y;
    }

    center_x += axis_x * snapshot.extent_a * EDGE_SCROLL_VIEWPORTS_PER_SECOND * dt;
    center_y += axis_y * snapshot.extent_b * EDGE_SCROLL_VIEWPORTS_PER_SECOND * dt;
    center_x = clamp_center(center_x, snapshot.extent_a);
    center_y = clamp_center(center_y, snapshot.extent_b);
    EDGE_CENTER_X.store(center_x.to_bits(), Ordering::Release);
    EDGE_CENTER_Y.store(center_y.to_bits(), Ordering::Release);
    unsafe { write_camera_center(snapshot, center_x, center_y) };
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
    LAST_SELECTED_ATHLETE.store(NO_ATHLETE, Ordering::Release);
    reset_manual_camera_state();
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
