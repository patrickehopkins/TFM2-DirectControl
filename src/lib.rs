mod camera_probe;

use std::sync::atomic::{AtomicBool, Ordering};

use mod_api_stable::{
    declare_stable_mod, ClientSceneKindV1, LogLevel, StableClient, StableExtension, StableHost,
    StableMod, TextAlignXV1, TextAlignYV1,
};
use windows_sys::Win32::{
    Foundation::{POINT, RECT},
    Graphics::Gdi::ScreenToClient,
    System::Threading::GetCurrentProcessId,
    UI::{
        Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON},
        WindowsAndMessaging::{
            GetClientRect, GetCursorPos, GetForegroundWindow, GetWindowThreadProcessId,
        },
    },
};

const MOD_ID: &str = "tfm2_direct_control";
const UI_FALLBACK_W: f32 = 1920.0;
const UI_FALLBACK_H: f32 = 1080.0;

static WAS_INGAME: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, Default)]
struct MouseSnapshot {
    valid: bool,
    ui_x: f32,
    ui_y: f32,
    left_down: bool,
    right_down: bool,
    client_w: i32,
    client_h: i32,
}

#[derive(Debug, Default)]
struct DirectControlExtension;

impl DirectControlExtension {
    fn read_mouse(&self, ctx: &StableClient<'_>) -> MouseSnapshot {
        // StableClient exposes raw keyboard input but not raw mouse coordinates/buttons.
        // Keep all Windows-specific input acquisition isolated here.
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_null() {
                return MouseSnapshot::default();
            }

            let mut foreground_process_id = 0u32;
            GetWindowThreadProcessId(hwnd, &mut foreground_process_id);
            if foreground_process_id == 0 || foreground_process_id != GetCurrentProcessId() {
                return MouseSnapshot::default();
            }

            let mut cursor = POINT { x: 0, y: 0 };
            if GetCursorPos(&mut cursor) == 0 || ScreenToClient(hwnd, &mut cursor) == 0 {
                return MouseSnapshot::default();
            }

            let mut rect = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            if GetClientRect(hwnd, &mut rect) == 0 {
                return MouseSnapshot::default();
            }

            let client_w = rect.right - rect.left;
            let client_h = rect.bottom - rect.top;
            if client_w <= 0 || client_h <= 0 {
                return MouseSnapshot::default();
            }

            // The game can remain foreground while the physical cursor crosses onto another
            // monitor. Off-client coordinates must never become gameplay commands.
            if cursor.x < 0 || cursor.y < 0 || cursor.x >= client_w || cursor.y >= client_h {
                return MouseSnapshot::default();
            }

            let (ui_w, ui_h) = ctx
                .draw_map_size("UI")
                .unwrap_or((UI_FALLBACK_W, UI_FALLBACK_H));

            MouseSnapshot {
                valid: true,
                ui_x: cursor.x as f32 * ui_w / client_w as f32,
                ui_y: cursor.y as f32 * ui_h / client_h as f32,
                left_down: GetAsyncKeyState(VK_LBUTTON as i32) < 0,
                right_down: GetAsyncKeyState(VK_RBUTTON as i32) < 0,
                client_w,
                client_h,
            }
        }
    }

    fn should_draw(ctx: &StableClient<'_>) -> bool {
        matches!(
            ctx.client_scene_kind(),
            Some(ClientSceneKindV1::Match | ClientSceneKindV1::InGame)
        )
    }

    fn draw_text_line(ctx: &mut StableClient<'_>, y: f32, text: &str, color: u32) {
        ctx.draw_text(
            "UI",
            text,
            "asset/base/font/set/regular",
            (28.0, y, 1_260.0, 22.0),
            19_999,
            13.0,
            color,
            TextAlignXV1::Left,
            TextAlignYV1::Center,
        );
    }

    fn draw_camera_probe(ctx: &mut StableClient<'_>) {
        let install = camera_probe::ensure_installed();
        let snapshots = camera_probe::snapshots();
        let rows = snapshots.len().max(1) as f32;
        let height = 32.0 + rows * 22.0;

        ctx.draw_rect("UI", 18.0, 58.0, 1_300.0, height, 19_998, 6.0, 0x101018dd);

        match install {
            Ok(()) => Self::draw_text_line(
                ctx,
                62.0,
                "Camera hook: INSTALLED (v0.5.8 signature verified)",
                0x80ff9fff,
            ),
            Err(error) => {
                Self::draw_text_line(ctx, 62.0, &format!("Camera hook: FAILED - {error}"), 0xff7070ff);
                return;
            }
        }

        if snapshots.is_empty() {
            Self::draw_text_line(
                ctx,
                84.0,
                "Waiting for the game's camera handler to run...",
                0xffffffff,
            );
            return;
        }

        for (index, camera) in snapshots.iter().enumerate() {
            let scaled_a = if camera.zoom != 0.0 {
                camera.extent_a / camera.zoom
            } else {
                0.0
            };
            let scaled_b = if camera.zoom != 0.0 {
                camera.extent_b / camera.zoom
            } else {
                0.0
            };
            let line = format!(
                "Cam{} 0x{:016X} | zoom {:.2} | center ({:.2}, {:.2}) | raw ext ({:.2}, {:.2}) | ext/zoom ({:.2}, {:.2}) | mode {} | calls {}",
                index + 1,
                camera.address,
                camera.zoom,
                camera.center_x,
                camera.center_y,
                camera.extent_a,
                camera.extent_b,
                scaled_a,
                scaled_b,
                camera.mode,
                camera.calls,
            );
            Self::draw_text_line(ctx, 84.0 + index as f32 * 22.0, &line, 0xffffffff);
        }
    }
}

impl StableExtension for DirectControlExtension {
    fn post_render(&self, ctx: &mut StableClient<'_>) {
        let ingame = matches!(ctx.client_scene_kind(), Some(ClientSceneKindV1::InGame));
        let was_ingame = WAS_INGAME.swap(ingame, Ordering::AcqRel);
        if ingame && !was_ingame {
            camera_probe::clear_candidates();
        }

        if !Self::should_draw(ctx) {
            return;
        }

        let mouse = self.read_mouse(ctx);
        if mouse.valid {
            let x = mouse.ui_x;
            let y = mouse.ui_y;
            let crosshair_color = if mouse.right_down {
                0xff4040ff
            } else if mouse.left_down {
                0x40ff80ff
            } else {
                0xffffffff
            };

            ctx.draw_line("UI", x - 14.0, y, x + 14.0, y, 2.0, 20_000, crosshair_color);
            ctx.draw_line("UI", x, y - 14.0, x, y + 14.0, 2.0, 20_000, crosshair_color);
            ctx.draw_circle("UI", x, y, 3.0, 20_001, crosshair_color);

            let label = format!(
                "TFM2 Direct Control | cursor UI ({:.1}, {:.1}) | client {}x{} | LMB {} | RMB {}",
                mouse.ui_x,
                mouse.ui_y,
                mouse.client_w,
                mouse.client_h,
                if mouse.left_down { "DOWN" } else { "up" },
                if mouse.right_down { "DOWN" } else { "up" },
            );

            ctx.draw_rect("UI", 18.0, 18.0, 900.0, 34.0, 19_998, 6.0, 0x101018dd);
            ctx.draw_text(
                "UI",
                &label,
                "asset/base/font/set/regular",
                (28.0, 18.0, 880.0, 34.0),
                19_999,
                14.0,
                0xffffffff,
                TextAlignXV1::Left,
                TextAlignYV1::Center,
            );
        }

        if ingame {
            Self::draw_camera_probe(ctx);
        }
    }
}

fn init(host: &StableHost) -> StableMod {
    host.log(
        LogLevel::Info,
        "TFM2 Direct Control loaded (mouse + version-checked live camera probe)",
    );

    let mut module = StableMod::new(MOD_ID);
    module.set_extension(DirectControlExtension);
    module
}

declare_stable_mod!(init);
