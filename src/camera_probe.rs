//! Version-checked camera-state capture for Teamfight Manager 2 v0.5.8.
//!
//! The official stable mod API deliberately does not expose the spectator camera.
//! For the single tested 0.5.8 executable we therefore detour the game's camera
//! input/update handler and capture only a few verified fields from its `this`
//! pointer.
//!
//! Direct Control camera movement uses the game's verified native pan-input fields
//! at +0x418/+0x41C so ordinary manual panning still goes through TFM2's camera
//! controller and can disengage follow/Auto Camera normally. Edge scrolling is pure
//! native pan input. MMB drag is cursor-anchored: the camera center is derived from
//! the total cursor displacement from the original press point, so a grabbed world
//! point follows the mouse 1:1 regardless of mouse speed or camera-handler cadence.
//!
//! Keep every version-specific RVA/offset in this module. Higher-level direct
//! control code should consume `CameraSnapshot` and never know TFM2's private
//! object layout.

use std::{
    ffi::c_void,
    ptr,
    sync::{
        atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering},
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

// Horizontal battlefield edges coincide closely with the client edges in the wide
// layout. Vertically, TFM2 reserves a top scoreboard band and a bottom controls band,
// so use a wider Y activation zone until camera input is moved onto live UI geometry.
const EDGE_SCROLL_MARGIN_X_PX: i32 = 18;
const EDGE_SCROLL_MARGIN_Y_PX: i32 = 72;
const NATIVE_EDGE_PAN_SPEED: f32 = 200.0;
const NATIVE_DRAG_INTENT_SPEED: f32 = 100.0;
const GAME_DRAW_SIZE: f32 = 2048.0;
const UI_FALLBACK_W: f32 = 1920.0;
const UI_FALLBACK_H: f32 = 1080.0;
const WORLD_MIN: f32 = 0.0;
const WORLD_MAX: f32 = 960.0;
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

#[derive(Debug, Clone, Copy)]
struct DragCorrection {
    center_x: f32,
    center_y: f32,
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
static PAN_WAS_ACTIVE: AtomicBool = AtomicBool::new(false);
static MMB_WAS_DOWN: AtomicBool = AtomicBool::new(false);
static DRAG_START_MOUSE_X: AtomicI32 = AtomicI32::new(0);
static DRAG_START_MOUSE_Y: AtomicI32 = AtomicI32::new(0);
static DRAG_LAST_MOUSE_X: AtomicI32 = AtomicI32::new(0);
static DRAG_LAST_MOUSE_Y: AtomicI32 = AtomicI32::new(0);
static DRAG_START_CENTER_X: AtomicU32 = AtomicU32::new(0);
static DRAG_START_CENTER_Y: AtomicU32 = AtomicU32::new(0);
static DRAG_START_EXTENT_X: AtomicU32 = AtomicU32::new(0);
static DRAG_START_EXTENT_Y: AtomicU32 = AtomicU32::new(0);
static DRAG_START_CLIENT_W: AtomicI32 = AtomicI32::new(0);
static DRAG_START_CLIENT_H: AtomicI32 = AtomicI32::new(0);

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
    let drag_correction = inject_native_pan(this);

    let trampoline = TRAMPOLINE.load(Ordering::Acquire);
    if trampoline != 0 {
        let original: CameraHandlerFn = std::mem::transmute(trampoline);
        original(this, arg2, arg3, arg4, arg5, arg6, arg7);
    }

    // MMB native pan is only an intent pulse to make the base controller see a
    // manual gesture. Do not leave a fixed velocity sitting in the camera object.
    if MMB_WAS_DOWN.load(Ordering::Acquire) {
        ptr::write_unaligned(this.add(PAN_X_OFFSET).cast::<f32>(), 0.0);
        ptr::write_unaligned(this.add(PAN_Y_OFFSET).cast::<f32>(), 0.0);
        PAN_WAS_ACTIVE.store(false, Ordering::Release);
    }

    if let Some(correction) = drag_correction {
        apply_drag_correction(this, correction);
    }

    capture(this);
}

fn direct_control_camera_active() -> bool {
    let selected = crate::control::selected_athlete().unwrap_or(NO_ATHLETE);
    let previous = LAST_SELECTED_ATHLETE.swap(selected, Ordering::AcqRel);
    if selected != previous {
        reset_manual_pan_state();
    }

    selected != NO_ATHLETE && crate::pacing_probe::manual_input_enabled()
}

fn reset_manual_pan_state() {
    PAN_WAS_ACTIVE.store(false, Ordering::Release);
    MMB_WAS_DOWN.store(false, Ordering::Release);
    DRAG_START_MOUSE_X.store(0, Ordering::Relaxed);
    DRAG_START_MOUSE_Y.store(0, Ordering::Relaxed);
    DRAG_LAST_MOUSE_X.store(0, Ordering::Relaxed);
    DRAG_LAST_MOUSE_Y.store(0, Ordering::Relaxed);
    DRAG_START_CENTER_X.store(0, Ordering::Relaxed);
    DRAG_START_CENTER_Y.store(0, Ordering::Relaxed);
    DRAG_START_EXTENT_X.store(0, Ordering::Relaxed);
    DRAG_START_EXTENT_Y.store(0, Ordering::Relaxed);
    DRAG_START_CLIENT_W.store(0, Ordering::Relaxed);
    DRAG_START_CLIENT_H.store(0, Ordering::Relaxed);
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

        Some((point.x, point.y, width, height))
    }
}

fn edge_axis(mouse_x: i32, mouse_y: i32, width: i32, height: i32) -> (f32, f32) {
    let left = mouse_x >= 0 && mouse_x <= EDGE_SCROLL_MARGIN_X_PX;
    let right = mouse_x < width && mouse_x >= width - EDGE_SCROLL_MARGIN_X_PX - 1;
    let up = mouse_y >= 0 && mouse_y <= EDGE_SCROLL_MARGIN_Y_PX;
    let down = mouse_y < height && mouse_y >= height - EDGE_SCROLL_MARGIN_Y_PX - 1;

    let mut x = (right as i32 - left as i32) as f32;
    let mut y = (down as i32 - up as i32) as f32;
    if x != 0.0 && y != 0.0 {
        const INV_SQRT_2: f32 = 0.707_106_77;
        x *= INV_SQRT_2;
        y *= INV_SQRT_2;
    }
    (x, y)
}

unsafe fn inject_native_pan(this: *mut u8) -> Option<DragCorrection> {
    if this.is_null() || !direct_control_camera_active() {
        reset_manual_pan_state();
        return None;
    }

    let Some((mouse_x, mouse_y, width, height)) = foreground_cursor() else {
        reset_manual_pan_state();
        return None;
    };

    let extent_a = ptr::read_unaligned(this.add(EXTENT_A_OFFSET).cast::<f32>());
    let extent_b = ptr::read_unaligned(this.add(EXTENT_B_OFFSET).cast::<f32>());
    let center_x = ptr::read_unaligned(this.add(CENTER_X_OFFSET).cast::<f32>());
    let center_y = ptr::read_unaligned(this.add(CENTER_Y_OFFSET).cast::<f32>());
    if !extent_a.is_finite()
        || !extent_b.is_finite()
        || !center_x.is_finite()
        || !center_y.is_finite()
        || extent_a <= 0.0
        || extent_b <= 0.0
    {
        return None;
    }

    let middle_down = GetAsyncKeyState(VK_MBUTTON_CODE) < 0;
    let mut pan_x = 0.0f32;
    let mut pan_y = 0.0f32;
    let mut correction = None;

    if middle_down {
        let was_down = MMB_WAS_DOWN.swap(true, Ordering::AcqRel);
        if !was_down {
            DRAG_START_MOUSE_X.store(mouse_x, Ordering::Release);
            DRAG_START_MOUSE_Y.store(mouse_y, Ordering::Release);
            DRAG_LAST_MOUSE_X.store(mouse_x, Ordering::Release);
            DRAG_LAST_MOUSE_Y.store(mouse_y, Ordering::Release);
            DRAG_START_CENTER_X.store(center_x.to_bits(), Ordering::Release);
            DRAG_START_CENTER_Y.store(center_y.to_bits(), Ordering::Release);
            DRAG_START_EXTENT_X.store(extent_a.to_bits(), Ordering::Release);
            DRAG_START_EXTENT_Y.store(extent_b.to_bits(), Ordering::Release);
            DRAG_START_CLIENT_W.store(width, Ordering::Release);
            DRAG_START_CLIENT_H.store(height, Ordering::Release);
        } else {
            let previous_x = DRAG_LAST_MOUSE_X.swap(mouse_x, Ordering::AcqRel);
            let previous_y = DRAG_LAST_MOUSE_Y.swap(mouse_y, Ordering::AcqRel);
            let sample_dx = mouse_x - previous_x;
            let sample_dy = mouse_y - previous_y;

            let start_x = DRAG_START_MOUSE_X.load(Ordering::Acquire);
            let start_y = DRAG_START_MOUSE_Y.load(Ordering::Acquire);
            let total_dx = mouse_x - start_x;
            let total_dy = mouse_y - start_y;
            let start_center_x = f32::from_bits(DRAG_START_CENTER_X.load(Ordering::Acquire));
            let start_center_y = f32::from_bits(DRAG_START_CENTER_Y.load(Ordering::Acquire));
            let start_extent_x = f32::from_bits(DRAG_START_EXTENT_X.load(Ordering::Acquire));
            let start_extent_y = f32::from_bits(DRAG_START_EXTENT_Y.load(Ordering::Acquire));
            let start_width = DRAG_START_CLIENT_W.load(Ordering::Acquire).max(1) as f32;
            let start_height = DRAG_START_CLIENT_H.load(Ordering::Acquire).max(1) as f32;

            if start_center_x.is_finite()
                && start_center_y.is_finite()
                && start_extent_x.is_finite()
                && start_extent_y.is_finite()
                && start_extent_x > 0.0
                && start_extent_y > 0.0
            {
                let ui_per_client_x = UI_FALLBACK_W / start_width;
                let ui_per_client_y = UI_FALLBACK_H / start_height;
                let world_dx = total_dx as f32 * ui_per_client_x * start_extent_x / GAME_DRAW_SIZE;
                let world_dy = total_dy as f32 * ui_per_client_y * start_extent_y / GAME_DRAW_SIZE;

                correction = Some(DragCorrection {
                    center_x: (start_center_x - world_dx).clamp(WORLD_MIN, WORLD_MAX),
                    center_y: (start_center_y - world_dy).clamp(WORLD_MIN, WORLD_MAX),
                });
            }

            // Give TFM2 a native manual-pan signal only while the mouse actually moves.
            // Distance never comes from this fixed input; the absolute correction above
            // is derived from total cursor displacement since the original hold point.
            pan_x = if sample_dx > 0 {
                -NATIVE_DRAG_INTENT_SPEED
            } else if sample_dx < 0 {
                NATIVE_DRAG_INTENT_SPEED
            } else {
                0.0
            };
            pan_y = if sample_dy > 0 {
                -NATIVE_DRAG_INTENT_SPEED
            } else if sample_dy < 0 {
                NATIVE_DRAG_INTENT_SPEED
            } else {
                0.0
            };
        }
    } else {
        MMB_WAS_DOWN.store(false, Ordering::Release);
        let (axis_x, axis_y) = edge_axis(mouse_x, mouse_y, width, height);
        pan_x = axis_x * NATIVE_EDGE_PAN_SPEED;
        pan_y = axis_y * NATIVE_EDGE_PAN_SPEED;
    }

    let active = pan_x != 0.0 || pan_y != 0.0;
    if active {
        ptr::write_unaligned(this.add(PAN_X_OFFSET).cast::<f32>(), pan_x);
        ptr::write_unaligned(this.add(PAN_Y_OFFSET).cast::<f32>(), pan_y);
        PAN_WAS_ACTIVE.store(true, Ordering::Release);
    } else if PAN_WAS_ACTIVE.swap(false, Ordering::AcqRel) {
        // Clear one stale injected input when the gesture ends. Do not continually
        // zero these fields: outside our gesture the base game's native input owns them.
        ptr::write_unaligned(this.add(PAN_X_OFFSET).cast::<f32>(), 0.0);
        ptr::write_unaligned(this.add(PAN_Y_OFFSET).cast::<f32>(), 0.0);
    }

    correction
}

unsafe fn apply_drag_correction(this: *mut u8, correction: DragCorrection) {
    if this.is_null() || !correction.center_x.is_finite() || !correction.center_y.is_finite() {
        return;
    }

    ptr::write_unaligned(this.add(CENTER_X_OFFSET).cast::<f32>(), correction.center_x);
    ptr::write_unaligned(this.add(CENTER_Y_OFFSET).cast::<f32>(), correction.center_y);
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
    reset_manual_pan_state();
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