//! Shared foreground-process gate for raw Win32 input polling.
//!
//! GetAsyncKeyState is global. Any Direct Control hotkey that reads it must first prove the
//! foreground window belongs to this TFM2 process; otherwise normal shortcuts typed in another
//! application can mutate the match in the background.

use windows_sys::Win32::{
    System::Threading::GetCurrentProcessId,
    UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
};

pub fn process_owns_foreground_window() -> bool {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return false;
        }

        let mut process_id = 0u32;
        GetWindowThreadProcessId(hwnd, &mut process_id);
        process_id != 0 && process_id == GetCurrentProcessId()
    }
}
