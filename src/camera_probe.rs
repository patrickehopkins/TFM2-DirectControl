//! Camera capture plus match-wide camera driving for known Teamfight Manager 2 builds.
//!
//! Keep capture and camera mutation deliberately separate. `base` owns the physically
//! validated native camera hook. Harbinger never writes the derived camera center at
//! +0xE4/+0xE8; doing that created a second camera authority and caused flicker,
//! snap-back, and screen-to-world disagreement.
//!
//! MMB drag publishes desired native pan values to `base`; the native hook applies them
//! synchronously immediately before TFM2's own camera handler runs. The game stays
//! authoritative for actual camera-center integration, bounds, follow state, minimap
//! camera jumps, and rendering.
//!
//! MMB drag and wheel zoom are match-view QoL, not manual-control ownership. They remain
//! available while spectating after `End` releases a champion.
//!
//! Screen-edge scrolling is deliberately shelved. Do not synthesize mouse movement or
//! post fake pointer messages to keep the camera alive; that experiment visibly disturbed
//! UI hover state. The only pointer-routing exception in this pass is a narrow MMB-only
//! shim: real physical WM_MOUSEMOVE events are forwarded to native UI at an inert
//! battlefield coordinate while Harbinger continues reading the true OS cursor itself.

#[path = "camera_probe/base.rs"]
mod base;

pub use base::{CameraSnapshot, FollowProbeReport};

use std::{
    ffi::c_void,
    sync::{
        atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering},
        Mutex, OnceLock,
    },
    thread,
    time::Duration,
};

const GAME_DRAW_SIZE: f32 = 2048.0;
const UI_FALLBACK_W: f32 = 1920.0;
const UI_FALLBACK_H: f32 = 1080.0;
const WORLD_MIN: f32 = 0.0;
const WORLD_MAX: f32 = 960.0;

// MMB is position-driven rather than velocity-driven. The cursor defines an exact
// desired camera center. A proportional native-pan command continuously closes the
// gap between TFM2's real captured center and that desired center. There is no drag
// speed ceiling: the same cursor displacement requests the same camera displacement
// regardless of how quickly the mouse moved.
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
const WM_MOUSEWHEEL: u32 = 0x020A;
const MK_LBUTTON: usize = 0x0001;
const MK_RBUTTON: usize = 0x0002;
const MK_MBUTTON: usize = 0x0010;
const WHEEL_DELTA: i32 = 120;

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
    fn SetWindowLongPtrW(hwnd: *mut c_void, index: i32, new_long: isize) -> isize;
    fn CallWindowProcW(
        previous: isize,
        hwnd: *mut c_void,
        message: u32,
        wparam: usize,
        lparam: isize,
    ) -> isize;
    fn DefWindowProcW(hwnd: *mut c_void, message: u32, wparam: usize, lparam: isize) -> isize;
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

// The window shim never generates messages of its own. During MMB it only changes the
// coordinates of real incoming mouse-move messages before native UI sees them. Harbinger
// reads the real pointer separately with GetCursorPos, so camera motion remains anchored
// to the physical mouse while cards/buttons/panels never become the native hover target.
static SHIM_HWND: AtomicUsize = AtomicUsize::new(0);
static ORIGINAL_WNDPROC: AtomicUsize = AtomicUsize::new(0);
static MATCH_CAMERA_ACTIVE: AtomicBool = AtomicBool::new(false);
static ACTIVE_CAMERA_ADDRESS: AtomicUsize = AtomicUsize::new(0);
static MMB_UI_LOCK_ACTIVE: AtomicBool = AtomicBool::new(false);
static VIRTUAL_HOVER_FLIP: AtomicBool = AtomicBool::new(false);
static SWALLOWED_LBUTTON: AtomicBool = AtomicBool::new(false);
static SWALLOWED_RBUTTON: AtomicBool = AtomicBool::new(false);
static WHEEL_REMAINDER: AtomicI32 = AtomicI32::new(0);

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

unsafe fn virtual_battlefield_lparam(hwnd: *mut c_void) -> Option<isize> {
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
    if width < 4 || height < 4 {
        return None;
    }

    // Quarter-width / half-height is inside the battlefield in both validated match
    // layouts. Alternate by one physical pixel only when a *real* mouse-move arrives;
    // this prevents an event framework from coalescing repeated identical coordinates
    // without creating any synthetic movement or touching the OS cursor.
    let flip = VIRTUAL_HOVER_FLIP.fetch_xor(true, Ordering::AcqRel);
    let base_x = (width / 4).clamp(1, width - 2);
    let x = (base_x + if flip { 1 } else { 0 }).clamp(1, width - 2);
    let y = (height / 2).clamp(1, height - 2);
    Some(pack_client_point(x, y))
}

fn wheel_steps(delta: i32) -> i32 {
    loop {
        let previous = WHEEL_REMAINDER.load(Ordering::Acquire);
        let total = previous.saturating_add(delta);
        let steps = total / WHEEL_DELTA;
        let remainder = total - steps * WHEEL_DELTA;
        if WHEEL_REMAINDER
            .compare_exchange(previous, remainder, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            return steps;
        }
    }
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

    if !MATCH_CAMERA_ACTIVE.load(Ordering::Acquire) {
        return CallWindowProcW(original, hwnd, message, wparam, lparam);
    }

    match message {
        WM_MBUTTONDOWN | WM_MBUTTONDBLCLK => {
            MMB_UI_LOCK_ACTIVE.store(true, Ordering::Release);
            // MMB has no native match function here; keep it exclusively as the camera
            // grab button so a widget cannot capture the drag on press.
            return 0;
        }
        WM_MBUTTONUP => {
            MMB_UI_LOCK_ACTIVE.store(false, Ordering::Release);
            SWALLOWED_LBUTTON.store(false, Ordering::Release);
            SWALLOWED_RBUTTON.store(false, Ordering::Release);
            // Do not synthesize a hover-restoration move. Native UI resumes on the next
            // genuine mouse message at the real pointer location.
            return 0;
        }
        WM_MOUSEMOVE if MMB_UI_LOCK_ACTIVE.load(Ordering::Acquire) => {
            if let Some(safe) = virtual_battlefield_lparam(hwnd) {
                let clean_wparam = wparam & !(MK_LBUTTON | MK_RBUTTON | MK_MBUTTON);
                return CallWindowProcW(original, hwnd, message, clean_wparam, safe);
            }
            return 0;
        }
        WM_LBUTTONDOWN | WM_LBUTTONDBLCLK if MMB_UI_LOCK_ACTIVE.load(Ordering::Acquire) => {
            // Direct Control still sees the physical button through GetAsyncKeyState.
            // Only native UI activation is suppressed while MMB owns pointer routing.
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
        WM_MOUSEWHEEL => {
            // Do not change zoom in the middle of an anchored MMB drag; that would alter
            // the screen/world scale underneath the captured drag origin.
            if MMB_UI_LOCK_ACTIVE.load(Ordering::Acquire) {
                return 0;
            }

            let delta = ((wparam >> 16) & 0xffff) as u16 as i16 as i32;
            let steps = wheel_steps(delta);
            let address = ACTIVE_CAMERA_ADDRESS.load(Ordering::Acquire);
            if address != 0 && steps != 0 {
                base::queue_zoom_steps(address, steps);
            }
            // Wheel is now a match-wide camera control; do not let a hovered widget
            // consume the same notch for unrelated UI behavior.
            return 0;
        }
        _ => {}
    }

    CallWindowProcW(original, hwnd, message, wparam, lparam)
}

fn release_pointer_shim() {
    let hwnd = SHIM_HWND.swap(0, Ordering::AcqRel);
    let original = ORIGINAL_WNDPROC.swap(0, Ordering::AcqRel);

    MMB_UI_LOCK_ACTIVE.store(false, Ordering::Release);
    VIRTUAL_HOVER_FLIP.store(false, Ordering::Release);
    SWALLOWED_LBUTTON.store(false, Ordering::Release);
    SWALLOWED_RBUTTON.store(false, Ordering::Release);
    WHEEL_REMAINDER.store(0, Ordering::Release);

    if hwnd != 0 && original != 0 {
        unsafe {
            let _ = SetWindowLongPtrW(hwnd as *mut c_void, GWLP_WNDPROC, original as isize);
        }
    }
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

fn clamp_center(value: f32) -> f32 {
    value.clamp(WORLD_MIN, WORLD_MAX)
}

fn clear_pan(state: &mut CameraControlState) {
    if state.pan_active && state.pan_address != 0 {
        base::clear_pan_request(state.pan_address);
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

fn publish_pan(state: &mut CameraControlState, address: usize, pan_x: f32, pan_y: f32) {
    if pan_x != 0.0 || pan_y != 0.0 {
        if state.pan_active && state.pan_address != 0 && state.pan_address != address {
            base::clear_pan_request(state.pan_address);
        }
        base::set_pan_request(address, pan_x, pan_y);
        state.pan_address = address;
        state.pan_active = true;
    } else {
        clear_pan(state);
    }
}

fn disable_match_camera(state: &mut CameraControlState) {
    MATCH_CAMERA_ACTIVE.store(false, Ordering::Release);
    ACTIVE_CAMERA_ADDRESS.store(0, Ordering::Release);
    reset_gesture(state);
    release_pointer_shim();
}

fn camera_control_step(state: &mut CameraControlState) {
    // MMB and wheel zoom remain available throughout the interactive match, regardless
    // of whether a champion is currently under manual control. Screen-edge panning is
    // intentionally absent from this driver while that feature is shelved.
    if !camera_match_active() {
        disable_match_camera(state);
        return;
    }

    let Some(camera) = preferred_snapshot() else {
        disable_match_camera(state);
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
        disable_match_camera(state);
        return;
    }

    let Some((hwnd, mouse_x, mouse_y, width, height)) = foreground_cursor() else {
        disable_match_camera(state);
        return;
    };

    ensure_pointer_shim(hwnd);
    ACTIVE_CAMERA_ADDRESS.store(camera.address, Ordering::Release);
    MATCH_CAMERA_ACTIVE.store(true, Ordering::Release);

    let middle_down = unsafe { GetAsyncKeyState(VK_MBUTTON_CODE) < 0 };
    MMB_UI_LOCK_ACTIVE.store(middle_down, Ordering::Release);

    if middle_down {
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
        let pan_x = (desired_x - camera.center_x) * DRAG_POSITION_GAIN;
        let pan_y = (desired_y - camera.center_y) * DRAG_POSITION_GAIN;
        publish_pan(state, camera.address, pan_x, pan_y);
    } else {
        state.middle_down = false;
        publish_pan(state, camera.address, 0.0, 0.0);
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
        disable_match_camera(&mut state);
    } else {
        MATCH_CAMERA_ACTIVE.store(false, Ordering::Release);
        ACTIVE_CAMERA_ADDRESS.store(0, Ordering::Release);
        release_pointer_shim();
    }
    base::clear_candidates();
}

/// Enforce the selected champion's native team vision while manual control is active.
///
/// TFM2 encodes spectator vision as 0=All, 1=Blue/team0, 2=Red/team1.
/// Passing None stops enforcement without changing the game's current view.
pub fn set_team_vision(team: Option<usize>) {
    let mode = team.and_then(|team| u8::try_from(team).ok()).and_then(|team| {
        let mode = team.saturating_add(1);
        (mode <= 2).then_some(mode)
    });
    base::set_vision_mode(mode);
}

pub fn follow_probe_report() -> Option<FollowProbeReport> {
    base::follow_probe_report()
}

pub fn snapshots() -> Vec<CameraSnapshot> {
    base::snapshots()
}
