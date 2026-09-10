mod camera_probe;
mod control;

use std::sync::atomic::{AtomicBool, Ordering};

use control::DirectControlAi;
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
const SIM_MAP_MAX: f32 = 960_000.0;
const CURSOR_WORLD_COLOR: u32 = 0xffd040ff;
const COMMAND_WORLD_COLOR: u32 = 0x40d8ffff;
const PLAYER_KEYS: [&str; 10] = ["F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10"];

static WAS_INGAME: AtomicBool = AtomicBool::new(false);
static LAST_RMB_DOWN: AtomicBool = AtomicBool::new(false);

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

            let left_down = GetAsyncKeyState(VK_LBUTTON as i32) < 0;
            let right_down = GetAsyncKeyState(VK_RBUTTON as i32) < 0;

            let mut cursor = POINT { x: 0, y: 0 };
            if GetCursorPos(&mut cursor) == 0 || ScreenToClient(hwnd, &mut cursor) == 0 {
                return MouseSnapshot {
                    left_down,
                    right_down,
                    ..Default::default()
                };
            }

            let mut rect = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            if GetClientRect(hwnd, &mut rect) == 0 {
                return MouseSnapshot {
                    left_down,
                    right_down,
                    ..Default::default()
                };
            }

            let client_w = rect.right - rect.left;
            let client_h = rect.bottom - rect.top;
            if client_w <= 0 || client_h <= 0 {
                return MouseSnapshot {
                    left_down,
                    right_down,
                    ..Default::default()
                };
            }

            // TFM2 can remain foreground while the physical cursor is on another monitor.
            // Keep button state for edge tracking, but never turn off-client coordinates into
            // a battlefield command.
            if cursor.x < 0 || cursor.y < 0 || cursor.x >= client_w || cursor.y >= client_h {
                return MouseSnapshot {
                    left_down,
                    right_down,
                    client_w,
                    client_h,
                    ..Default::default()
                };
            }

            let (ui_w, ui_h) = ctx
                .draw_map_size("UI")
                .unwrap_or((UI_FALLBACK_W, UI_FALLBACK_H));

            MouseSnapshot {
                valid: true,
                ui_x: cursor.x as f32 * ui_w / client_w as f32,
                ui_y: cursor.y as f32 * ui_h / client_h as f32,
                left_down,
                right_down,
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

    fn handle_player_selection(ctx: &StableClient<'_>) {
        for (player_id, key) in PLAYER_KEYS.iter().enumerate() {
            if ctx.key_pressed(key) {
                control::select_player(player_id);
                break;
            }
        }
    }

    fn draw_camera_and_controls(
        ctx: &mut StableClient<'_>,
        mouse: MouseSnapshot,
        right_pressed: bool,
    ) {
        let install = camera_probe::ensure_installed();
        let snapshots = camera_probe::snapshots();

        ctx.draw_rect("UI", 18.0, 58.0, 1_560.0, 134.0, 19_998, 6.0, 0x101018dd);

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

        let selection_text = match control::selected_player() {
            Some(player_id) => format!(
                "DIRECT CONTROL: F{} -> player_id {} selected | RMB battlefield = move | unselected players keep vanilla AI",
                player_id + 1,
                player_id
            ),
            None => "DIRECT CONTROL: press F1-F10 to select a player; no AI is overridden until selected".to_owned(),
        };
        Self::draw_text_line(ctx, 84.0, &selection_text, 0x80d8ffff);

        let Some(camera) = snapshots.iter().max_by_key(|candidate| candidate.calls) else {
            Self::draw_text_line(
                ctx,
                106.0,
                "Waiting for the game's camera handler to run...",
                0xffffffff,
            );
            return;
        };

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
                "Camera zoom {:.2} center ({:.2},{:.2}) extent ({:.2},{:.2}) | viewport ({vx:.1},{vy:.1},{vw:.1},{vh:.1}) | cursor inside {}",
                camera.zoom,
                camera.center_x,
                camera.center_y,
                camera.extent_a,
                camera.extent_b,
                if inside { "YES" } else { "no" }
            ),
            0xffffffff,
        );

        // Match the stable Game drawing camera to the live TFM2 camera before drawing either
        // the cursor projection or the last commanded destination.
        ctx.draw_set_camera(
            "Game",
            camera.center_x,
            camera.center_y,
            camera.extent_a,
            camera.extent_b,
        );

        let marker_units_per_px = ((camera.extent_a / game_w) + (camera.extent_b / game_h)) * 0.5;
        if let Some((target_x, target_y)) = control::move_target() {
            let target_world_x = target_x as f32 / SIM_UNITS_PER_CAMERA_UNIT;
            let target_world_y = target_y as f32 / SIM_UNITS_PER_CAMERA_UNIT;
            ctx.draw_circle(
                "Game",
                target_world_x,
                target_world_y,
                13.0 * marker_units_per_px,
                100_010,
                COMMAND_WORLD_COLOR,
            );
            ctx.draw_circle(
                "Game",
                target_world_x,
                target_world_y,
                5.0 * marker_units_per_px,
                100_011,
                COMMAND_WORLD_COLOR,
            );
        }

        if !inside || game_w <= 0.0 || game_h <= 0.0 {
            Self::draw_text_line(
                ctx,
                128.0,
                "Cursor is outside the battlefield; RMB will not issue a movement command.",
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
        let in_map_bounds = (0.0..=SIM_MAP_MAX).contains(&sim_x)
            && (0.0..=SIM_MAP_MAX).contains(&sim_y);

        ctx.draw_circle(
            "Game",
            world_x,
            world_y,
            11.0 * marker_units_per_px,
            100_000,
            CURSOR_WORLD_COLOR,
        );
        ctx.draw_circle(
            "Game",
            world_x,
            world_y,
            7.0 * marker_units_per_px,
            100_001,
            CURSOR_WORLD_COLOR,
        );

        if right_pressed && in_map_bounds && control::selected_player().is_some() {
            control::publish_move_target(sim_x.round() as u64, sim_y.round() as u64);
        }

        let command_status = if right_pressed {
            if control::selected_player().is_none() {
                "RMB ignored: select F1-F10 first"
            } else if !in_map_bounds {
                "RMB ignored: projected point is outside the 960000x960000 map"
            } else {
                "RMB MOVE ISSUED"
            }
        } else if control::selected_player().is_some() {
            "ready for RMB move"
        } else {
            "select F1-F10"
        };

        Self::draw_text_line(
            ctx,
            128.0,
            &format!(
                "cursor world ({world_x:.2},{world_y:.2}) | sim ({sim_x:.0},{sim_y:.0}) | map bounds {} | {command_status}",
                if in_map_bounds { "YES" } else { "no" }
            ),
            if right_pressed && in_map_bounds && control::selected_player().is_some() {
                0x80ff9fff
            } else {
                0xffffffff
            },
        );

        let last_target = control::move_target()
            .map(|(x, y)| format!("last move target ({x},{y})"))
            .unwrap_or_else(|| "no move target yet".to_owned());
        Self::draw_text_line(
            ctx,
            150.0,
            &format!("TEST: selected champion should stop under manual control and path to each RMB destination | {last_target}"),
            COMMAND_WORLD_COLOR,
        );
    }
}

impl StableExtension for DirectControlExtension {
    fn post_render(&self, ctx: &mut StableClient<'_>) {
        let ingame = matches!(ctx.client_scene_kind(), Some(ClientSceneKindV1::InGame));
        let was_ingame = WAS_INGAME.swap(ingame, Ordering::AcqRel);

        if ingame && !was_ingame {
            camera_probe::clear_candidates();
            control::reset();
            LAST_RMB_DOWN.store(false, Ordering::Release);
        } else if !ingame && was_ingame {
            control::reset();
            LAST_RMB_DOWN.store(false, Ordering::Release);
        }

        if !Self::should_draw(ctx) {
            return;
        }

        if ingame {
            Self::handle_player_selection(ctx);
        }

        let mouse = self.read_mouse(ctx);
        let previous_right = LAST_RMB_DOWN.swap(mouse.right_down, Ordering::AcqRel);
        let right_pressed = ingame && mouse.right_down && !previous_right;

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
            Self::draw_camera_and_controls(ctx, mouse, right_pressed);
        }
    }
}

fn init(host: &StableHost) -> StableMod {
    host.log(
        LogLevel::Info,
        "TFM2 Direct Control loaded (first manual movement build; single-player diagnostic)",
    );

    let mut module = StableMod::new(MOD_ID);
    module.add_player_input_ai(DirectControlAi::default());
    module.set_extension(DirectControlExtension);
    module
}

declare_stable_mod!(init);
