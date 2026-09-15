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

#[path = "camera_probe/base.rs"]
mod base;

pub use base::CameraSnapshot;

use std::{
    ffi::c_void,
    ptr,
    sync::{Mutex, OnceLock},
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

// MMB is position-driven rather than velocity-driven. The cursor defines an exact
// desired camera center. A proportional native-pan command continuously closes the
// gap between TFM2's *real captured center* and that desired center. At a 60 Hz camera
// update this gain corrects approximately one frame of position error per update;
// faster updates converge in multiple smaller steps without any hard speed ceiling.
const DRAG_POSITION_GAIN: f32 = 60.0;
const CAMERA_CONTROL_POLL_MS: u64 = 2;
const VK_MBUTTON_CODE: i32 = 0x04;

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
        }
    }
}

static DRIVER_STARTED: OnceLock<()> = OnceLock::new();
static DRIVER_STATE: OnceLock<Mutex<CameraControlState>> = OnceLock::new();

fn driver_state() -> &'static Mutex<CameraControlState> {
    DRIVER_STATE.get_or_init(|| Mutex::new(CameraControlState::default()))
}

fn preferred_snapshot() -> Option<CameraSnapshot> {
    base::snapshots()
        .into_iter()
        .max_by_key(|candidate| candidate.calls)
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
}

fn camera_match_active() -> bool {
    crate::pacing_probe::snapshot().interactive_match
}

fn camera_control_step(state: &mut CameraControlState) {
    // Camera gestures remain active throughout the interactive match, regardless of
    // whether a champion is currently under manual control. Releasing with `End`
    // therefore returns combat to spectator/AI control without sacrificing MMB/edge QoL.
    if !camera_match_active() {
        reset_gesture(state);
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

    let Some((mouse_x, mouse_y, width, height)) = foreground_cursor() else {
        reset_gesture(state);
        return;
    };

    let middle_down = unsafe { GetAsyncKeyState(VK_MBUTTON_CODE) < 0 };
    let (pan_x, pan_y) = if middle_down {
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
    base::clear_candidates();
}

pub fn snapshots() -> Vec<CameraSnapshot> {
    base::snapshots()
}
