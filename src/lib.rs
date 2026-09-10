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
const SIM_UNITS_PER_CAMERA_UNIT: f32 = 1000.0;
const GAME_MARKER_COLOR: u32 = 0xffd040ff;

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
            (28.0, y, 1_500.0, 22.0),
            19_999,
            13.0,
            color,
            TextAlignXV1::Left,
            TextAlignYV1::Center,
        );
    }

    fn point_in_rect(px: f32, py: f32, rect: (f32, f32, f32, f32)) -> bool {
        let (x, y, w, h) = rect;
        px >= x && py >= y && px < x + w && py < y + h
    }

    fn draw_camera_probe(ctx: &mut StableClient<'_>, mouse: MouseSnapshot) {
        let install = camera_probe::ensure_installed();
        let snapshots = camera_probe::snapshots();

        ctx.draw_rect("UI", 18.0, 58.0, 1_560.0, 112.0, 19_998, 6.0, 0x101018dd);

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

        let Some(camera) = snapshots.iter().max_by_key(|candidate| candidate.calls) else {
            Self::draw_text_line(
                ctx,
                84.0,
                "Waiting for the game's camera handler to run...",
                0xffffffff,
            );
            return;
        };

        Self::draw_text_line(
            ctx,
            84.0,
            &format!(
                "Camera 0x{:016X} | candidates {} | zoom {:.2} | center ({:.2}, {:.2}) | extent ({:.2}, {:.2}) | mode {} | calls {}",
                camera.address,
                snapshots.len(),
                camera.zoom,
                camera.center_x,
                camera.center_y,
                camera.extent_a,
                camera.extent_b,
                camera.mode,
                camera.calls,
            ),
            0xffffffff,
        );

        let Some(viewport) = ctx.ui_node_rect("ingame.center_log") else {
            Self::draw_text_line(ctx, 106.0, "Viewport: ingame.center_log unavailable", 0xffd080ff);
            return;
        };
        let Some((game_w, game_h)) = ctx.draw_map_size("Game") else {
            Self::draw_text_line(ctx, 106.0, "Game render-map size unavailable", 0xffd080ff);
            return;
        };

        let (vx, vy, vw, vh) = viewport;
        let viewport_cx = vx + vw * 0.5;
        let viewport_cy = vy + vh * 0.5;
        let inside = mouse.valid && Self::point_in_rect(mouse.ui_x, mouse.ui_y, viewport);

        Self::draw_text_line(
            ctx,
            106.0,
            &format!(
                "Viewport ({vx:.1},{vy:.1},{vw:.1},{vh:.1}) center ({viewport_cx:.1},{viewport_cy:.1}) | Game {game_w:.0}x{game_h:.0} | cursor inside {}",
                if inside { "YES" } else { "no" }
            ),
            0xffffffff,
        );

        if !inside || game_w <= 0.0 || game_h <= 0.0 {
            Self::draw_text_line(
                ctx,
                128.0,
                "Move the cursor over the battlefield to test UI -> camera world projection.",
                0xffd080ff,
            );
            return;
        }

        let dx = mouse.ui_x - viewport_cx;
        let dy = mouse.ui_y - viewport_cy;
        let units_per_px_x = camera.extent_a / game_w;
        let units_per_px_y = camera.extent_b / game_h;
        let world_x = camera.center_x + dx * units_per_px_x;
        let world_y = camera.center_y + dy * units_per_px_y;
        let sim_x = world_x * SIM_UNITS_PER_CAMERA_UNIT;
        let sim_y = world_y * SIM_UNITS_PER_CAMERA_UNIT;

        // The stable API defines "Game" as match-world space. The previous diagnostic
        // accidentally treated its 2048x2048 backing map as raw screen pixels, which only
        // aligned at 0.50x because the live camera extent also happened to be 2048 there.
        // Match the stable draw camera to TFM2's captured live camera, then draw at the
        // calculated world coordinate. If the projection is correct, these rings remain
        // centered on the UI-space cursor at every zoom level.
        ctx.draw_set_camera(
            "Game",
            camera.center_x,
            camera.center_y,
            camera.extent_a,
            camera.extent_b,
        );
        let marker_units_per_px = (units_per_px_x + units_per_px_y) * 0.5;
        ctx.draw_circle(
            "Game",
            world_x,
            world_y,
            11.0 * marker_units_per_px,
            100_000,
            GAME_MARKER_COLOR,
        );
        ctx.draw_circle(
            "Game",
            world_x,
            world_y,
            7.0 * marker_units_per_px,
            100_001,
            GAME_MARKER_COLOR,
        );

        Self::draw_text_line(
            ctx,
            128.0,
            &format!(
                "delta UI ({dx:.1},{dy:.1}) | units/px ({units_per_px_x:.5},{units_per_px_y:.5}) | world ({world_x:.2},{world_y:.2}) | sim ({sim_x:.0},{sim_y:.0})"
            ),
            0xffffffff,
        );
        Self::draw_text_line(
            ctx,
            150.0,
            "TEST v2: yellow world-space rings should remain centered on the white cursor crosshair at every zoom/layout.",
            GAME_MARKER_COLOR,
        );
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
            Self::draw_camera_probe(ctx, mouse);
        }
    }
}

fn init(host: &StableHost) -> StableMod {
    host.log(
        LogLevel::Info,
        "TFM2 Direct Control loaded (camera projection validation v2)",
    );

    let mut module = StableMod::new(MOD_ID);
    module.set_extension(DirectControlExtension);
    module
}

declare_stable_mod!(init);
