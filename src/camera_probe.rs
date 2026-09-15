//! Camera capture plus match-wide camera driving for Teamfight Manager 2 v0.5.8.
//!
//! Keep capture and camera mutation deliberately separate. `base` is the physically
//! validated read-only camera adapter. Harbinger never writes the derived camera
//! center at +0xE4/+0xE8; doing that created a second camera authority and caused
//! flicker, snap-back, and screen-to-world disagreement. Camera gestures drive only
//! TFM2's native pan inputs at +0x418/+0x41C, leaving the game authoritative for the
//! actual camera center, follow state, minimap camera jumps, bounds, and rendering.
//!
//! These camera gestures are match-view QoL, not manual-control ownership. MMB drag
//! and edge scroll remain available while spectating after `End` releases a champion.
//!
//! TFM2's UI pointer routing can otherwise stall camera integration while the cursor
//! crosses interactive widgets. During MMB drag, a narrow window-procedure shim hides
//! the real pointer from native UI hit testing while Harbinger continues reading the
//! true Win32 cursor for camera motion and gameplay clicks. Edge scroll periodically
//! posts a harmless battlefield mouse-move pulse so a stationary physical edge hover
//! follows the same smooth native update path as a moving cursor.

#[path = "camera_probe/base.rs"]
mod base;

pub use base::CameraSnapshot;

use std::{
    ffi::c_void,
    ptr,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Mutex, OnceLock,
    },
    thread,
    time::Duration,
};

const PAN_X_OFFSET: usize = 0x418;
const PAN_Y_OFFSET: usize = 0x41C;
const GAME_DRAW_SIZE: f32 = 2048.0;
const UI_FALLBACK_W: f32 = 1920.0;
const UI_FALLBACK_H: f32 = 1080.0;
const WORLD_MIN: f32 = 0.0;
const WORLD_MAX: f32 = 960.0;

const EDGE_SCROLL_MARGIN_X_PX: i32 = 18;
const EDGE_SCROLL_MARGIN_Y_PX: i32 = 84;
const NATIVE_EDGE_PAN_SPEED: f32 = 200.0;
const EDGE_WAKE_INTERVAL_MS: u64 = 8;

// MMB is position-driven rather than velocity-driven. The cursor defines an exact
// desired camera center. A proportional native-pan command continuously closes the
// gap between TFM2's *real captured center* and that desired center. At a 60 Hz camera
// update this gain corrects approximately one frame of position error per update;
// faster updates converge in multiple smaller steps without any hard speed ceiling.
const DRAG_POSITION_GAIN: f32 = 60.0;
const CAMERA_CONTROL_POLL_MS: u64 = 2;
const VK_MBUTTON_CODE: i32 = 0x04;

const GWLP_WNDPROC: i32 = -4;
const WM_MOUSEMOVE: u32 = 0x0200;
const WM_LBUTTONDOWN: u32 = 0x0201;
const WM_LBUTTONUP: u32 = 0x0202;
const WM_LBUTTONDBLCLK: u32 = 0x0203;
const WM_RBUTTONDOWN: u32 = 0x0204;
const WM_RBUTTONUP: u32 = 0x0205;
const WM_RBUTTONDBLCLK: u32 = 0x0206;
const WM_MBUTTONDOWN: u32 = 0x0207;
const WM_MBUTTONUP: u32 = 0x0208;
const WM_MBUTTONDBLCLK: u32 = 0x0209;
const MK_LBUTTON: usize = 0x0001;
const MK_RBUTTON: usize = 0x0002;
const MK_MBUTTON: usize = 0x0010;

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
    fn GetCurrentProcessId() -> u32;
    fn GetTickCount64() -> u64;
}

#[link(name = "user32")]
extern "system" {
    fn GetAsyncKeyState(vkey: i32) -> i16;
    fn GetForegroundWindow() -> *mut c_void;
    fn GetWindowThreadProcessId(hwnd: *mut c_void, process_id: *mut u32) -> u32;
    fn GetCursorPos(point: *mut WinPoint) -> i32;
    fn ScreenToClient(hwnd: *mut c_void, point: *mut WinPoint) -> i32;
    fn GetClientRect(hwnd: *mut c_void, rect: *mut WinRect) -> i32;
    fn SetWindowLongPtrW(hwnd: *mut c_void, index: i32, new_long: isize) -> isize;
    fn CallWindowProcW(
        previous: isize,
        hwnd: *mut c_void,
        message: u32,
        wparam: usize,
        lparam: isize,
    ) -> isize;
    fn DefWindowProcW(hwnd: *mut c_void, message: u32, wparam: usize, lparam: isize) -> isize;
    fn PostMessageW(hwnd: *mut c_void, message: u32, wparam: usize, lparam: isize) -> i32;
}

#[derive(Debug, Clone, Copy)]
struct CameraControlState {
    pan_address: usize,
    pan_active: bool,
    middle_down: bool,
    drag_start_mouse_x: i32,
    drag_start_mouse_y: i32,
    drag_start_center_x: f32,
    drag_start_center_y: f32,
    drag_start_extent_x: f32,
    drag_start_extent_y: f32,
    drag_start_client_w: i32,
    drag_start_client_h: i32,
    last_edge_wake_ms: u64,
    edge_wake_flip: bool,
}

impl Default for CameraControlState {
    fn default() -> Self {
        Self {
            pan_address: 0,
            pan_active: false,
            middle_down: false,
            drag_start_mouse_x: 0,
            drag_start_mouse_y: 0,
            drag_start_center_x: 0.0,
            drag_start_center_y: 0.0,
            drag_start_extent_x: 0.0,
            drag_start_extent_y: 0.0,
            drag_start_client_w: 1,
            drag_start_client_h: 1,
            last_edge_wake_ms: 0,
            edge_wake_flip: false,
        }
    }
}

static DRIVER_STARTED: OnceLock<()> = OnceLock::new();
static DRIVER_STATE: OnceLock<Mutex<CameraControlState>> = OnceLock::new();

// UI-pointer shim state. The shim exists only while an interactive match is active.
// It does not move the OS cursor. Harbinger's gameplay input continues to use
// GetCursorPos/GetAsyncKeyState directly, so hiding MMB from TFM2's UI does not hide
// ground/champion clicks from Direct Control.
static SHIM_HWND: AtomicUsize = AtomicUsize::new(0);
static ORIGINAL_WNDPROC: AtomicUsize = AtomicUsize::new(0);
static MMB_UI_LOCK_ACTIVE: AtomicBool = AtomicBool::new(false);
static SWALLOWED_LBUTTON: AtomicBool = AtomicBool::new(false);
static SWALLOWED_RBUTTON: AtomicBool = AtomicBool::new(false);

fn driver_state() -> &'static Mutex<CameraControlState> {
    DRIVER_STATE.get_or_init(|| Mutex::new(CameraControlState::default()))
}

fn preferred_snapshot() -> Option<CameraSnapshot> {
    base::snapshots()
        .into_iter()
        .max_by_key(|candidate| candidate.calls)
}

fn pack_client_point(x: i32, y: i32) -> isize {
    let packed_x = x as i16 as u16 as u32;
    let packed_y = y as i16 as u16 as u32;
    (packed_x | (packed_y << 16)) as isize
}

unsafe fn safe_pointer_lparam(hwnd: *mut c_void, flip: bool) -> Option<isize> {
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

    // Quarter-width is safely inside the battlefield in both the full and split/info
    // layouts. Alternate by one pixel for synthetic edge wakes so frameworks that
    // collapse identical mouse-move coordinates still observe motion.
    let base_x = (width / 4).clamp(1, width - 2);
    let x = (base_x + if flip { 1 } else { 0 }).clamp(1, width - 2);
    let y = (height / 2).clamp(1, height - 2);
    Some(pack_client_point(x, y))
}

unsafe extern "system" fn camera_pointer_wndproc(
    hwnd: *mut c_void,
    message: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    let original = ORIGINAL_WNDPROC.load(Ordering::Acquire) as isize;
    if original == 0 {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }

    match message {
        WM_MBUTTONDOWN | WM_MBUTTONDBLCLK => {
            MMB_UI_LOCK_ACTIVE.store(true, Ordering::Release);
            if let Some(safe) = safe_pointer_lparam(hwnd, false) {
                let clean_wparam = wparam & !(MK_LBUTTON | MK_RBUTTON | MK_MBUTTON);
                let _ = CallWindowProcW(original, hwnd, WM_MOUSEMOVE, clean_wparam, safe);
            }
            // MMB belongs exclusively to camera drag. Do not let a minimap/card/panel
            // capture it or alter the UI pointer state.
            return 0;
        }
        WM_MBUTTONUP => {
            MMB_UI_LOCK_ACTIVE.store(false, Ordering::Release);
            // Restore native hover at the real release point immediately after the
            // camera gesture ends. The actual release coordinates are already in lparam.
            let clean_wparam = wparam & !(MK_LBUTTON | MK_RBUTTON | MK_MBUTTON);
            let _ = CallWindowProcW(original, hwnd, WM_MOUSEMOVE, clean_wparam, lparam);
            return 0;
        }
        WM_LBUTTONDOWN | WM_LBUTTONDBLCLK if MMB_UI_LOCK_ACTIVE.load(Ordering::Acquire) => {
            SWALLOWED_LBUTTON.store(true, Ordering::Release);
            return 0;
        }
        WM_LBUTTONUP if SWALLOWED_LBUTTON.swap(false, Ordering::AcqRel) => {
            return 0;
        }
        WM_RBUTTONDOWN | WM_RBUTTONDBLCLK if MMB_UI_LOCK_ACTIVE.load(Ordering::Acquire) => {
            SWALLOWED_RBUTTON.store(true, Ordering::Release);
            return 0;
        }
        WM_RBUTTONUP if SWALLOWED_RBUTTON.swap(false, Ordering::AcqRel) => {
            return 0;
        }
        WM_MOUSEMOVE if MMB_UI_LOCK_ACTIVE.load(Ordering::Acquire) => {
            if let Some(safe) = safe_pointer_lparam(hwnd, false) {
                let clean_wparam = wparam & !(MK_LBUTTON | MK_RBUTTON | MK_MBUTTON);
                return CallWindowProcW(original, hwnd, message, clean_wparam, safe);
            }
        }
        _ => {}
    }

    CallWindowProcW(original, hwnd, message, wparam, lparam)
}

fn ensure_pointer_shim(hwnd: usize) {
    if hwnd == 0 || SHIM_HWND.load(Ordering::Acquire) == hwnd {
        return;
    }

    release_pointer_shim();

    unsafe {
        let previous = SetWindowLongPtrW(
            hwnd as *mut c_void,
            GWLP_WNDPROC,
            camera_pointer_wndproc as usize as isize,
        );
        if previous != 0 {
            ORIGINAL_WNDPROC.store(previous as usize, Ordering::Release);
            SHIM_HWND.store(hwnd, Ordering::Release);
        }
    }
}

fn release_pointer_shim() {
    let hwnd = SHIM_HWND.swap(0, Ordering::AcqRel);
    let original = ORIGINAL_WNDPROC.swap(0, Ordering::AcqRel);
    MMB_UI_LOCK_ACTIVE.store(false, Ordering::Release);
    SWALLOWED_LBUTTON.store(false, Ordering::Release);
    SWALLOWED_RBUTTON.store(false, Ordering::Release);

    if hwnd != 0 && original != 0 {
        unsafe {
            let _ = SetWindowLongPtrW(hwnd as *mut c_void, GWLP_WNDPROC, original as isize);
        }
    }
}

fn foreground_cursor() -> Option<(usize, i32, i32, i32, i32)> {
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

        Some((hwnd as usize, point.x, point.y, width, height))
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

fn clamp_center(value: f32) -> f32 {
    value.clamp(WORLD_MIN, WORLD_MAX)
}

unsafe fn write_native_pan(address: usize, pan_x: f32, pan_y: f32) {
    if address == 0 || !pan_x.is_finite() || !pan_y.is_finite() {
        return;
    }
    let this = address as *mut u8;
    ptr::write_unaligned(this.add(PAN_X_OFFSET).cast::<f32>(), pan_x);
    ptr::write_unaligned(this.add(PAN_Y_OFFSET).cast::<f32>(), pan_y);
}

fn clear_pan(state: &mut CameraControlState) {
    if state.pan_active && state.pan_address != 0 {
        unsafe { write_native_pan(state.pan_address, 0.0, 0.0) };
    }
    state.pan_active = false;
    state.pan_address = 0;
}

fn reset_gesture(state: &mut CameraControlState) {
    clear_pan(state);
    state.middle_down = false;
    state.last_edge_wake_ms = 0;
}

fn camera_match_active() -> bool {
    crate::pacing_probe::snapshot().interactive_match
}

fn post_edge_wake(state: &mut CameraControlState, hwnd: usize) {
    let now_ms = unsafe { GetTickCount64() };
    if state.last_edge_wake_ms != 0
        && now_ms.saturating_sub(state.last_edge_wake_ms) < EDGE_WAKE_INTERVAL_MS
    {
        return;
    }

    state.last_edge_wake_ms = now_ms;
    state.edge_wake_flip = !state.edge_wake_flip;
    unsafe {
        if let Some(safe) = safe_pointer_lparam(hwnd as *mut c_void, state.edge_wake_flip) {
            let _ = PostMessageW(hwnd as *mut c_void, WM_MOUSEMOVE, 0, safe);
        }
    }
}

fn camera_control_step(state: &mut CameraControlState) {
    // Camera gestures remain active throughout the interactive match, regardless of
    // whether a champion is currently under manual control. Releasing with `End`
    // therefore returns combat to spectator/AI control without sacrificing MMB/edge QoL.
    if !camera_match_active() {
        reset_gesture(state);
        release_pointer_shim();
        return;
    }

    let Some(camera) = preferred_snapshot() else {
        reset_gesture(state);
        return;
    };
    if camera.address == 0
        || !camera.center_x.is_finite()
        || !camera.center_y.is_finite()
        || !camera.extent_a.is_finite()
        || !camera.extent_b.is_finite()
        || camera.extent_a <= 0.0
        || camera.extent_b <= 0.0
    {
        reset_gesture(state);
        return;
    }

    let Some((hwnd, mouse_x, mouse_y, width, height)) = foreground_cursor() else {
        reset_gesture(state);
        return;
    };
    ensure_pointer_shim(hwnd);

    let middle_down = unsafe { GetAsyncKeyState(VK_MBUTTON_CODE) < 0 };
    MMB_UI_LOCK_ACTIVE.store(middle_down, Ordering::Release);

    let (pan_x, pan_y) = if middle_down {
        state.last_edge_wake_ms = 0;
        if !state.middle_down {
            state.middle_down = true;
            state.drag_start_mouse_x = mouse_x;
            state.drag_start_mouse_y = mouse_y;
            state.drag_start_center_x = camera.center_x;
            state.drag_start_center_y = camera.center_y;
            state.drag_start_extent_x = camera.extent_a;
            state.drag_start_extent_y = camera.extent_b;
            state.drag_start_client_w = width.max(1);
            state.drag_start_client_h = height.max(1);
        }

        let dx = mouse_x - state.drag_start_mouse_x;
        let dy = mouse_y - state.drag_start_mouse_y;
        let ui_per_client_x = UI_FALLBACK_W / state.drag_start_client_w as f32;
        let ui_per_client_y = UI_FALLBACK_H / state.drag_start_client_h as f32;
        let world_dx = dx as f32 * ui_per_client_x * state.drag_start_extent_x / GAME_DRAW_SIZE;
        let world_dy = dy as f32 * ui_per_client_y * state.drag_start_extent_y / GAME_DRAW_SIZE;

        let desired_x = clamp_center(state.drag_start_center_x - world_dx);
        let desired_y = clamp_center(state.drag_start_center_y - world_dy);
        (
            (desired_x - camera.center_x) * DRAG_POSITION_GAIN,
            (desired_y - camera.center_y) * DRAG_POSITION_GAIN,
        )
    } else {
        state.middle_down = false;
        let (axis_x, axis_y) = edge_axis(mouse_x, mouse_y, width, height);
        if axis_x != 0.0 || axis_y != 0.0 {
            // The native camera consumes our pan fields smoothly when mouse movement
            // causes the game to run its pointer/update path. Generate that cadence
            // ourselves while the physical cursor is stationary at an edge, using a
            // safe battlefield coordinate so top/bottom UI bars cannot suppress it.
            post_edge_wake(state, hwnd);
        } else {
            state.last_edge_wake_ms = 0;
        }
        (
            axis_x * NATIVE_EDGE_PAN_SPEED,
            axis_y * NATIVE_EDGE_PAN_SPEED,
        )
    };

    if pan_x != 0.0 || pan_y != 0.0 {
        if state.pan_active && state.pan_address != 0 && state.pan_address != camera.address {
            unsafe { write_native_pan(state.pan_address, 0.0, 0.0) };
        }
        unsafe { write_native_pan(camera.address, pan_x, pan_y) };
        state.pan_address = camera.address;
        state.pan_active = true;
    } else {
        clear_pan(state);
    }
}

fn camera_control_loop() {
    loop {
        if let Ok(mut state) = driver_state().lock() {
            camera_control_step(&mut state);
        }
        thread::sleep(Duration::from_millis(CAMERA_CONTROL_POLL_MS));
    }
}

fn ensure_driver_started() {
    DRIVER_STARTED.get_or_init(|| {
        let _ = thread::Builder::new()
            .name("tfm2-direct-control-camera".to_owned())
            .spawn(camera_control_loop);
    });
}

pub fn ensure_installed() -> Result<(), String> {
    base::ensure_installed()?;
    ensure_driver_started();
    Ok(())
}

pub fn clear_candidates() {
    if let Ok(mut state) = driver_state().lock() {
        reset_gesture(&mut state);
    }
    release_pointer_shim();
    base::clear_candidates();
}

pub fn snapshots() -> Vec<CameraSnapshot> {
    base::snapshots()
}
