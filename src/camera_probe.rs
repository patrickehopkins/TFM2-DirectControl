//! Version-checked camera-state capture and Direct Control camera gestures for
//! Teamfight Manager 2 v0.5.8.
//!
//! The stable API does not expose the spectator camera, so this module detours the
//! validated native camera handler to capture the live camera object. Direct Control
//! edge scroll and MMB drag are sampled independently from the game's input-event
//! cadence so stationary edge hover and fast drags do not depend on mouse-move events.
//!
//! MMB is an anchored grab: camera center is derived from total cursor displacement
//! from the original press point. Edge scroll integrates continuously from wall-clock
//! time. Once Direct Control manually moves the camera, Harbinger latches the chosen
//! center and reapplies it synchronously after TFM2's native camera update. This is
//! important: writing the visible center only from the background gesture thread lets
//! the native controller restore its old center between writes, producing rapid
//! flicker and a snap-back when the gesture ends.

use std::{
    ffi::c_void,
    ptr,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering},
        OnceLock,
    },
    thread,
    time::Duration,
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
const UI_FALLBACK_W: f32 = 1920.0;
const UI_FALLBACK_H: f32 = 1080.0;
const WORLD_MIN: f32 = 0.0;
const WORLD_MAX: f32 = 960.0;

const EDGE_SCROLL_MARGIN_X_PX: i32 = 18;
const EDGE_SCROLL_MARGIN_Y_PX: i32 = 84;
const EDGE_SCROLL_VIEWPORTS_PER_SECOND: f32 = 1.35;
const MAX_CONTROL_DT_SECONDS: f32 = 0.050;
const CAMERA_CONTROL_POLL_MS: u64 = 2;
const VK_MBUTTON_CODE: i32 = 0x04;
const NO_ATHLETE: usize = usize::MAX;

#[repr(C)]
struct WinPoint {
    x: i32,
    y: i32,
}

#[repr(C)]
struct WinRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(module_name: *const u16) -> *mut c_void;
    fn GetCurrentProcess() -> *mut c_void;
    fn GetCurrentProcessId() -> u32;
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
    fn GetForegroundWindow() -> *mut c_void;
    fn GetWindowThreadProcessId(hwnd: *mut c_void, process_id: *mut u32) -> u32;
    fn GetCursorPos(point: *mut WinPoint) -> i32;
    fn ScreenToClient(hwnd: *mut c_void, point: *mut WinPoint) -> i32;
    fn GetClientRect(hwnd: *mut c_void, rect: *mut WinRect) -> i32;
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

// Manual-center publication is a tiny seqlock. The gesture thread can update X/Y
// every couple of milliseconds while the native camera hook reads the pair. The
// sequence keeps the hook from ever combining X from one sample with Y from another.
static MANUAL_CENTER_ACTIVE: AtomicBool = AtomicBool::new(false);
static MANUAL_CENTER_ADDRESS: AtomicUsize = AtomicUsize::new(0);
static MANUAL_CENTER_X: AtomicU32 = AtomicU32::new(0);
static MANUAL_CENTER_Y: AtomicU32 = AtomicU32::new(0);
static MANUAL_CENTER_SEQ: AtomicU64 = AtomicU64::new(0);

type CameraHandlerFn = unsafe extern "system" fn(
    *mut u8,
    usize,
    usize,
    f32,
    usize,
    usize,
    usize,
);

fn manual_center_for(address: usize) -> Option<(f32, f32)> {
    if !MANUAL_CENTER_ACTIVE.load(Ordering::Acquire) {
        return None;
    }

    // The writer's critical section is only a few atomic stores. A short spin here
    // is preferable to letting one native camera update visibly restore the old
    // center for a frame.
    for _ in 0..64 {
        let before = MANUAL_CENTER_SEQ.load(Ordering::Acquire);
        if before & 1 != 0 {
            std::hint::spin_loop();
            continue;
        }

        let published_address = MANUAL_CENTER_ADDRESS.load(Ordering::Acquire);
        let x = f32::from_bits(MANUAL_CENTER_X.load(Ordering::Acquire));
        let y = f32::from_bits(MANUAL_CENTER_Y.load(Ordering::Acquire));
        let after = MANUAL_CENTER_SEQ.load(Ordering::Acquire);

        if before == after && after & 1 == 0 {
            if published_address == address && x.is_finite() && y.is_finite() {
                return Some((x, y));
            }
            return None;
        }
    }

    None
}

fn publish_manual_center(address: usize, center_x: f32, center_y: f32) {
    if address == 0 || !center_x.is_finite() || !center_y.is_finite() {
        return;
    }

    MANUAL_CENTER_SEQ.fetch_add(1, Ordering::AcqRel);
    MANUAL_CENTER_ADDRESS.store(address, Ordering::Relaxed);
    MANUAL_CENTER_X.store(center_x.to_bits(), Ordering::Relaxed);
    MANUAL_CENTER_Y.store(center_y.to_bits(), Ordering::Relaxed);
    MANUAL_CENTER_SEQ.fetch_add(1, Ordering::Release);
    MANUAL_CENTER_ACTIVE.store(true, Ordering::Release);
}

/// Return camera ownership to TFM2. This is intentionally public so native camera
/// actions (F-key follow, minimap camera jump, later Space follow) can explicitly
/// drop the Direct Control free-camera latch when those integrations are wired in.
pub fn release_manual_camera() {
    MANUAL_CENTER_ACTIVE.store(false, Ordering::Release);
    MANUAL_CENTER_ADDRESS.store(0, Ordering::Release);
}

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

    // TFM2 may have restored its own target/follow center during the native update.
    // Reapply Harbinger's latched manual center *inside the same camera update* so
    // render never sees the transient native center. This is the synchronization
    // missing from the previous build and is what prevents flicker/snap-back.
    if let Some((center_x, center_y)) = manual_center_for(this as usize) {
        ptr::write_unaligned(this.add(CENTER_X_OFFSET).cast::<f32>(), center_x);
        ptr::write_unaligned(this.add(CENTER_Y_OFFSET).cast::<f32>(), center_y);
    }

    capture(this);
}

#[derive(Debug, Clone, Copy)]
struct CameraControlState {
    selected_athlete: usize,
    middle_down: bool,
    drag_start_mouse_x: i32,
    drag_start_mouse_y: i32,
    drag_start_center_x: f32,
    drag_start_center_y: f32,
    drag_start_extent_x: f32,
    drag_start_extent_y: f32,
    drag_start_client_w: i32,
    drag_start_client_h: i32,
    edge_active: bool,
    edge_center_x: f32,
    edge_center_y: f32,
    last_step_ms: u64,
}

impl Default for CameraControlState {
    fn default() -> Self {
        Self {
            selected_athlete: NO_ATHLETE,
            middle_down: false,
            drag_start_mouse_x: 0,
            drag_start_mouse_y: 0,
            drag_start_center_x: 0.0,
            drag_start_center_y: 0.0,
            drag_start_extent_x: 0.0,
            drag_start_extent_y: 0.0,
            drag_start_client_w: 0,
            drag_start_client_h: 0,
            edge_active: false,
            edge_center_x: 0.0,
            edge_center_y: 0.0,
            last_step_ms: 0,
        }
    }
}

impl CameraControlState {
    fn reset_gesture(&mut self) {
        self.middle_down = false;
        self.edge_active = false;
        self.last_step_ms = 0;
    }
}

fn direct_control_camera_active(state: &mut CameraControlState) -> bool {
    let selected = crate::control::selected_athlete().unwrap_or(NO_ATHLETE);
    if selected != state.selected_athlete {
        state.selected_athlete = selected;
        state.reset_gesture();
        release_manual_camera();
    }

    if selected == NO_ATHLETE || !crate::pacing_probe::manual_input_enabled() {
        state.reset_gesture();
        release_manual_camera();
        return false;
    }

    true
}

fn foreground_cursor() -> Option<(i32, i32, i32, i32)> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }

        let mut process_id = 0u32;
        GetWindowThreadProcessId(hwnd, &mut process_id);
        if process_id == 0 || process_id != GetCurrentProcessId() {
            return None;
        }

        let mut point = WinPoint { x: 0, y: 0 };
        if GetCursorPos(&mut point) == 0 || ScreenToClient(hwnd, &mut point) == 0 {
            return None;
        }

        let mut rect = WinRect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if GetClientRect(hwnd, &mut rect) == 0 {
            return None;
        }

        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        if width <= 0 || height <= 0 {
            return None;
        }

        if point.x < 0 || point.y < 0 || point.x >= width || point.y >= height {
            return None;
        }

        Some((point.x, point.y, width, height))
    }
}

fn edge_axis(mouse_x: i32, mouse_y: i32, width: i32, height: i32) -> (f32, f32) {
    let left = mouse_x <= EDGE_SCROLL_MARGIN_X_PX;
    let right = mouse_x >= width - EDGE_SCROLL_MARGIN_X_PX - 1;
    let up = mouse_y <= EDGE_SCROLL_MARGIN_Y_PX;
    let down = mouse_y >= height - EDGE_SCROLL_MARGIN_Y_PX - 1;

    let mut x = (right as i32 - left as i32) as f32;
    let mut y = (down as i32 - up as i32) as f32;
    if x != 0.0 && y != 0.0 {
        const INV_SQRT_2: f32 = 0.707_106_77;
        x *= INV_SQRT_2;
        y *= INV_SQRT_2;
    }
    (x, y)
}

// TFM2 legitimately permits camera extents larger than the 960x960 logical map at
// low zoom. Extent-based clamping therefore collapses to center; clamp only the
// logical camera center itself.
fn clamp_center(center: f32) -> f32 {
    center.clamp(WORLD_MIN, WORLD_MAX)
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

unsafe fn write_camera_center(snapshot: CameraSnapshot, center_x: f32, center_y: f32) {
    if snapshot.address == 0 || !center_x.is_finite() || !center_y.is_finite() {
        return;
    }

    if !CANDIDATES
        .iter()
        .any(|slot| slot.address.load(Ordering::Acquire) == snapshot.address)
    {
        return;
    }

    // Publish first so a native camera update racing this thread already knows the
    // authoritative Direct Control center it must restore before returning.
    publish_manual_center(snapshot.address, center_x, center_y);

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

fn camera_control_step(state: &mut CameraControlState) {
    if !direct_control_camera_active(state) {
        return;
    }

    let Some(snapshot) = preferred_snapshot() else {
        state.reset_gesture();
        return;
    };
    if !snapshot.center_x.is_finite()
        || !snapshot.center_y.is_finite()
        || !snapshot.extent_a.is_finite()
        || !snapshot.extent_b.is_finite()
        || snapshot.extent_a <= 0.0
        || snapshot.extent_b <= 0.0
    {
        state.reset_gesture();
        return;
    }

    let Some((mouse_x, mouse_y, width, height)) = foreground_cursor() else {
        state.reset_gesture();
        return;
    };

    let now_ms = unsafe { GetTickCount64() };
    let dt = if state.last_step_ms == 0 {
        0.0
    } else {
        (now_ms.saturating_sub(state.last_step_ms) as f32 / 1000.0)
            .clamp(0.0, MAX_CONTROL_DT_SECONDS)
    };
    state.last_step_ms = now_ms;

    let middle_down = unsafe { GetAsyncKeyState(VK_MBUTTON_CODE) < 0 };
    if middle_down {
        state.edge_active = false;

        if !state.middle_down {
            state.middle_down = true;
            state.drag_start_mouse_x = mouse_x;
            state.drag_start_mouse_y = mouse_y;
            state.drag_start_center_x = snapshot.center_x;
            state.drag_start_center_y = snapshot.center_y;
            state.drag_start_extent_x = snapshot.extent_a;
            state.drag_start_extent_y = snapshot.extent_b;
            state.drag_start_client_w = width.max(1);
            state.drag_start_client_h = height.max(1);
            return;
        }

        let dx = mouse_x - state.drag_start_mouse_x;
        let dy = mouse_y - state.drag_start_mouse_y;
        let ui_per_client_x = UI_FALLBACK_W / state.drag_start_client_w.max(1) as f32;
        let ui_per_client_y = UI_FALLBACK_H / state.drag_start_client_h.max(1) as f32;
        let world_dx =
            dx as f32 * ui_per_client_x * state.drag_start_extent_x / GAME_DRAW_SIZE;
        let world_dy =
            dy as f32 * ui_per_client_y * state.drag_start_extent_y / GAME_DRAW_SIZE;

        let center_x = clamp_center(state.drag_start_center_x - world_dx);
        let center_y = clamp_center(state.drag_start_center_y - world_dy);
        unsafe { write_camera_center(snapshot, center_x, center_y) };
        return;
    }

    if state.middle_down {
        state.middle_down = false;
    }

    let (axis_x, axis_y) = edge_axis(mouse_x, mouse_y, width, height);
    let edge_active = axis_x != 0.0 || axis_y != 0.0;
    if !edge_active {
        state.edge_active = false;
        return;
    }

    if !state.edge_active {
        state.edge_active = true;
        state.edge_center_x = snapshot.center_x;
        state.edge_center_y = snapshot.center_y;
    }

    if dt <= 0.0 {
        return;
    }

    state.edge_center_x +=
        axis_x * snapshot.extent_a * EDGE_SCROLL_VIEWPORTS_PER_SECOND * dt;
    state.edge_center_y +=
        axis_y * snapshot.extent_b * EDGE_SCROLL_VIEWPORTS_PER_SECOND * dt;
    state.edge_center_x = clamp_center(state.edge_center_x);
    state.edge_center_y = clamp_center(state.edge_center_y);
    unsafe { write_camera_center(snapshot, state.edge_center_x, state.edge_center_y) };
}

fn camera_control_loop() {
    let mut state = CameraControlState::default();
    loop {
        camera_control_step(&mut state);
        thread::sleep(Duration::from_millis(CAMERA_CONTROL_POLL_MS));
    }
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
    release_manual_camera();
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

    let _ = thread::Builder::new()
        .name("tfm2-direct-control-camera".to_owned())
        .spawn(camera_control_loop);

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
