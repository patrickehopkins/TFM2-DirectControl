mod camera_probe;
mod pacing_probe;
mod simulation_probe;

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
const CURSOR_WORLD_COLOR: u32 = 0xffd040ff;

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
                return MouseSnapshot::default();
            }

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
            (28.0, y, 1_760.0, 22.0),
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

    fn draw_probe(&self, ctx: &mut StableClient<'_>, mouse: MouseSnapshot) {
        ctx.draw_rect("UI", 18.0, 58.0, 1_800.0, 250.0, 19_998, 6.0, 0x101018dd);

        match simulation_probe::ensure_installed() {
            Ok(()) => Self::draw_text_line(
                ctx,
                62.0,
                "SIM TASK PROBE: A confirmed watched-match job | Candidate-A AI observer READ ONLY",
                0x80ff9fff,
            ),
            Err(error) => {
                Self::draw_text_line(
                    ctx,
                    62.0,
                    &format!("SIM TASK PROBE: FAILED - {error}"),
                    0xff7070ff,
                );
                return;
            }
        }

        for (index, probe) in simulation_probe::snapshots().iter().enumerate() {
            Self::draw_text_line(
                ctx,
                84.0 + index as f32 * 22.0,
                &format!(
                    "{} RVA 0x{:X} | enter {} active {} done {} | thread {} | ctx 0x{:X} | last {} ms max {} ms",
                    probe.name,
                    probe.rva,
                    probe.entries,
                    probe.active,
                    probe.completions,
                    probe.last_thread_id,
                    probe.last_context,
                    probe.last_duration_ms,
                    probe.max_duration_ms,
                ),
                0xffd080ff,
            );
        }

        if let Ok(sig) = simulation_probe::core_signatures() {
            Self::draw_text_line(
                ctx,
                150.0,
                &format!(
                    "CORE WRAPPER RVA 0x{:X} first32: {}",
                    sig.wrapper_rva, sig.wrapper_bytes
                ),
                0xffd080ff,
            );
            Self::draw_text_line(
                ctx,
                172.0,
                &format!(
                    "CORE RUNNER  RVA 0x{:X} first32: {}",
                    sig.runner_rva, sig.runner_bytes
                ),
                0xffd080ff,
            );
        }

        let clock = ctx
            .ui_text("ingame.header.game_time.value")
            .unwrap_or_else(|| "--:--".to_owned());
        match camera_probe::ensure_installed() {
            Ok(()) => {
                let snapshots = camera_probe::snapshots();
                if let Some(camera) = snapshots.iter().max_by_key(|candidate| candidate.calls) {
                    Self::draw_text_line(
                        ctx,
                        194.0,
                        &format!(
                            "visible clock {} | camera calls {} mode {} zoom {:.2} center ({:.2},{:.2})",
                            clock,
                            camera.calls,
                            camera.mode,
                            camera.zoom,
                            camera.center_x,
                            camera.center_y,
                        ),
                        0xffffffff,
                    );

                    if let (Some(viewport), Some((game_w, game_h))) =
                        (ctx.ui_node_rect("ingame.center_log"), ctx.draw_map_size("Game"))
                    {
                        if mouse.valid
                            && game_w > 0.0
                            && game_h > 0.0
                            && Self::point_in_rect(mouse.ui_x, mouse.ui_y, viewport)
                        {
                            let (vx, vy, vw, vh) = viewport;
                            let dx = mouse.ui_x - (vx + vw * 0.5);
                            let dy = mouse.ui_y - (vy + vh * 0.5);
                            let world_x = camera.center_x + dx * (camera.extent_a / game_w);
                            let world_y = camera.center_y + dy * (camera.extent_b / game_h);
                            let marker_units_per_px =
                                ((camera.extent_a / game_w) + (camera.extent_b / game_h)) * 0.5;
                            ctx.draw_set_camera(
                                "Game",
                                camera.center_x,
                                camera.center_y,
                                camera.extent_a,
                                camera.extent_b,
                            );
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
                        }
                    }
                }
            }
            Err(error) => Self::draw_text_line(
                ctx,
                194.0,
                &format!("visible clock {} | camera hook failed: {error}", clock),
                0xff7070ff,
            ),
        }

        let pacing = pacing_probe::snapshot();
        let first_tick = pacing
            .first_candidate_a_tick
            .map(|tick| tick.to_string())
            .unwrap_or_else(|| "--".to_owned());
        let last_tick = pacing
            .last_candidate_a_tick
            .map(|tick| tick.to_string())
            .unwrap_or_else(|| "--".to_owned());
        let last_player = pacing
            .last_candidate_a_player
            .map(|player| player.to_string())
            .unwrap_or_else(|| "--".to_owned());

        Self::draw_text_line(
            ctx,
            216.0,
            &format!(
                "AI OBSERVER: total {} | Candidate A {} | thread {} | players mask 0x{:X}",
                pacing.total_think_calls,
                pacing.candidate_a_think_calls,
                pacing.last_candidate_a_thread,
                pacing.seen_player_mask,
            ),
            0x80d8ffff,
        );
        Self::draw_text_line(
            ctx,
            238.0,
            &format!(
                "Candidate A ctx.tick(): {} -> {} | last player {} | no sleep, no InputV1 mutation",
                first_tick, last_tick, last_player,
            ),
            0x80d8ffff,
        );
        Self::draw_text_line(
            ctx,
            260.0,
            "TEST TARGET: Candidate-A callback count/tick should advance only while A is active; playback must remain unchanged.",
            0x80d8ffff,
        );
    }
}

impl StableExtension for DirectControlExtension {
    fn post_render(&self, ctx: &mut StableClient<'_>) {
        let ingame = matches!(ctx.client_scene_kind(), Some(ClientSceneKindV1::InGame));
        let was_ingame = WAS_INGAME.swap(ingame, Ordering::AcqRel);
        if ingame && !was_ingame {
            camera_probe::clear_candidates();
            pacing_probe::reset();
        }

        if !Self::should_draw(ctx) {
            return;
        }

        let mouse = self.read_mouse(ctx);
        if mouse.valid {
            let crosshair_color = if mouse.right_down {
                0xff4040ff
            } else if mouse.left_down {
                0x40ff80ff
            } else {
                0xffffffff
            };
            ctx.draw_line(
                "UI",
                mouse.ui_x - 14.0,
                mouse.ui_y,
                mouse.ui_x + 14.0,
                mouse.ui_y,
                2.0,
                20_000,
                crosshair_color,
            );
            ctx.draw_line(
                "UI",
                mouse.ui_x,
                mouse.ui_y - 14.0,
                mouse.ui_x,
                mouse.ui_y + 14.0,
                2.0,
                20_000,
                crosshair_color,
            );
            ctx.draw_circle(
                "UI",
                mouse.ui_x,
                mouse.ui_y,
                3.0,
                20_001,
                crosshair_color,
            );

            ctx.draw_rect("UI", 18.0, 18.0, 900.0, 34.0, 19_998, 6.0, 0x101018dd);
            Self::draw_text_line(
                ctx,
                24.0,
                &format!(
                    "TFM2 Direct Control | cursor UI ({:.1},{:.1}) | client {}x{} | LMB {} | RMB {}",
                    mouse.ui_x,
                    mouse.ui_y,
                    mouse.client_w,
                    mouse.client_h,
                    if mouse.left_down { "DOWN" } else { "up" },
                    if mouse.right_down { "DOWN" } else { "up" },
                ),
                0xffffffff,
            );
        }

        if ingame {
            self.draw_probe(ctx, mouse);
        }
    }
}

fn init(host: &StableHost) -> StableMod {
    match simulation_probe::ensure_installed() {
        Ok(()) => host.log(
            LogLevel::Info,
            "TFM2 Direct Control loaded (Candidate A task probe + read-only AI observer)",
        ),
        Err(error) => host.log(
            LogLevel::Error,
            &format!("TFM2 Direct Control simulation task probe failed: {error}"),
        ),
    }

    let mut module = StableMod::new(MOD_ID);
    module.add_player_input_ai(pacing_probe::CandidateAObserverAi::default());
    module.set_extension(DirectControlExtension);
    module
}

declare_stable_mod!(init);
