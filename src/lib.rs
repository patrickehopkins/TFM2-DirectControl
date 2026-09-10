mod camera_probe;
mod control;
mod pacing_probe;
mod simulation_probe;

use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};

use mod_api_stable::{
    declare_stable_mod, ClientSceneKindV1, LogLevel, StableClient, StableExtension, StableHost,
    StableMod, TextAlignXV1, TextAlignYV1,
};
use windows_sys::Win32::{
    Foundation::{POINT, RECT},
    Graphics::Gdi::ScreenToClient,
    System::Threading::GetCurrentProcessId,
    UI::{
        Input::KeyboardAndMouse::{
            GetAsyncKeyState, VK_CONTROL, VK_END, VK_LBUTTON, VK_RBUTTON,
        },
        WindowsAndMessaging::{
            GetClientRect, GetCursorPos, GetForegroundWindow, GetWindowThreadProcessId,
        },
    },
};

const MOD_ID: &str = "tfm2_direct_control";
const UI_FALLBACK_W: f32 = 1920.0;
const UI_FALLBACK_H: f32 = 1080.0;
const CURSOR_WORLD_COLOR: u32 = 0xffd040ff;
const VK_F1_CODE: i32 = 0x70;
const PLAYER_SLOT_COUNT: usize = 10;
const SIM_UNITS_PER_WORLD_UNIT: f32 = 1000.0;

static WAS_INGAME: AtomicBool = AtomicBool::new(false);
static FINISH_CHORD_WAS_DOWN: AtomicBool = AtomicBool::new(false);
static SELECT_KEYS_WERE_DOWN: AtomicU16 = AtomicU16::new(0);
static RMB_WAS_DOWN: AtomicBool = AtomicBool::new(false);

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

#[derive(Debug, Clone, Copy)]
struct CursorWorld {
    world_x: f32,
    world_y: f32,
    sim_x: u64,
    sim_y: u64,
    marker_units_per_px: f32,
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

    fn poll_finish_chord(ingame: bool) {
        if !ingame {
            FINISH_CHORD_WAS_DOWN.store(false, Ordering::Release);
            return;
        }

        let chord_down = unsafe {
            GetAsyncKeyState(VK_CONTROL as i32) < 0 && GetAsyncKeyState(VK_END as i32) < 0
        };
        let was_down = FINISH_CHORD_WAS_DOWN.swap(chord_down, Ordering::AcqRel);
        if chord_down && !was_down {
            pacing_probe::request_finish_simulation();
        }
    }

    fn poll_player_selection(ingame: bool) {
        if !ingame {
            SELECT_KEYS_WERE_DOWN.store(0, Ordering::Release);
            return;
        }

        let mut down_mask = 0u16;
        for index in 0..PLAYER_SLOT_COUNT {
            let vk = VK_F1_CODE + index as i32;
            if unsafe { GetAsyncKeyState(vk) } < 0 {
                down_mask |= 1u16 << index;
            }
        }

        let previous = SELECT_KEYS_WERE_DOWN.swap(down_mask, Ordering::AcqRel);
        if pacing_probe::manual_control_released() {
            return;
        }

        let rising = down_mask & !previous;
        if rising != 0 {
            let slot = rising.trailing_zeros() as usize;
            // Deliberately team-agnostic: F1-F10 address raw player slots 0-9. Ownership/team
            // policy belongs to higher-level mods, not the direct-control primitive.
            control::select_player(slot);
        }
    }

    fn cursor_world(
        ctx: &StableClient<'_>,
        mouse: MouseSnapshot,
        camera: camera_probe::CameraSnapshot,
    ) -> Option<CursorWorld> {
        if !mouse.valid {
            return None;
        }

        let (ui_w, ui_h) = ctx
            .draw_map_size("UI")
            .unwrap_or((UI_FALLBACK_W, UI_FALLBACK_H));
        let (game_w, game_h) = ctx.draw_map_size("Game")?;
        if ui_w <= 0.0 || ui_h <= 0.0 || game_w <= 0.0 || game_h <= 0.0 {
            return None;
        }

        // The Game draw-map is the camera's render surface. Project the logical UI cursor onto
        // that full surface instead of depending on a particular UI node such as center_log.
        // Higher-level click/UI policy can later decide which screen regions are actionable.
        let game_x = mouse.ui_x * game_w / ui_w;
        let game_y = mouse.ui_y * game_h / ui_h;
        let dx = game_x - game_w * 0.5;
        let dy = game_y - game_h * 0.5;

        let world_x = camera.center_x + dx * (camera.extent_a / game_w);
        let world_y = camera.center_y + dy * (camera.extent_b / game_h);
        if !world_x.is_finite() || !world_y.is_finite() || world_x < 0.0 || world_y < 0.0 {
            return None;
        }

        // StableSim/InputV1 positions are fixed-point simulation coordinates at 1000 units per
        // camera/world unit. Stage 4A incorrectly omitted this conversion.
        let sim_x_f = world_x * SIM_UNITS_PER_WORLD_UNIT;
        let sim_y_f = world_y * SIM_UNITS_PER_WORLD_UNIT;
        if !sim_x_f.is_finite() || !sim_y_f.is_finite() || sim_x_f < 0.0 || sim_y_f < 0.0 {
            return None;
        }

        let marker_units_per_px =
            ((camera.extent_a / game_w) + (camera.extent_b / game_h)) * 0.5;

        Some(CursorWorld {
            world_x,
            world_y,
            sim_x: sim_x_f.round() as u64,
            sim_y: sim_y_f.round() as u64,
            marker_units_per_px,
        })
    }

    fn best_camera() -> Option<camera_probe::CameraSnapshot> {
        camera_probe::snapshots()
            .iter()
            .max_by_key(|candidate| candidate.calls)
            .copied()
    }

    fn poll_rmb_move(&self, ctx: &StableClient<'_>, mouse: MouseSnapshot, ingame: bool) {
        if !ingame {
            RMB_WAS_DOWN.store(false, Ordering::Release);
            return;
        }

        let was_down = RMB_WAS_DOWN.swap(mouse.right_down, Ordering::AcqRel);
        if !mouse.right_down
            || was_down
            || pacing_probe::manual_control_released()
            || control::selected_player().is_none()
        {
            return;
        }

        let Some(camera) = Self::best_camera() else {
            return;
        };
        let Some(cursor) = Self::cursor_world(ctx, mouse, camera) else {
            return;
        };

        control::publish_move_target(cursor.sim_x, cursor.sim_y);
    }

    fn draw_probe(&self, ctx: &mut StableClient<'_>, mouse: MouseSnapshot) {
        ctx.draw_rect("UI", 18.0, 58.0, 1_800.0, 338.0, 19_998, 6.0, 0x101018dd);

        match simulation_probe::ensure_installed() {
            Ok(()) => Self::draw_text_line(
                ctx,
                62.0,
                "SIM TASK PROBE: A confirmed watched-match job | Stage 4B team-neutral RMB MoveTo",
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
                &format!("CORE WRAPPER RVA 0x{:X} first32: {}", sig.wrapper_rva, sig.wrapper_bytes),
                0xffd080ff,
            );
            Self::draw_text_line(
                ctx,
                172.0,
                &format!("CORE RUNNER  RVA 0x{:X} first32: {}", sig.runner_rva, sig.runner_bytes),
                0xffd080ff,
            );
        }

        let clock = ctx
            .ui_text("ingame.header.game_time.value")
            .unwrap_or_else(|| "--:--".to_owned());

        let mut cursor_projection = None;
        match camera_probe::ensure_installed() {
            Ok(()) => {
                if let Some(camera) = Self::best_camera() {
                    Self::draw_text_line(
                        ctx,
                        194.0,
                        &format!(
                            "visible clock {} | camera calls {} mode {} zoom {:.2} center ({:.2},{:.2})",
                            clock, camera.calls, camera.mode, camera.zoom, camera.center_x, camera.center_y,
                        ),
                        0xffffffff,
                    );

                    cursor_projection = Self::cursor_world(ctx, mouse, camera);
                    if let Some(cursor) = cursor_projection {
                        ctx.draw_set_camera(
                            "Game",
                            camera.center_x,
                            camera.center_y,
                            camera.extent_a,
                            camera.extent_b,
                        );
                        ctx.draw_circle(
                            "Game",
                            cursor.world_x,
                            cursor.world_y,
                            11.0 * cursor.marker_units_per_px,
                            100_000,
                            CURSOR_WORLD_COLOR,
                        );
                        ctx.draw_circle(
                            "Game",
                            cursor.world_x,
                            cursor.world_y,
                            7.0 * cursor.marker_units_per_px,
                            100_001,
                            CURSOR_WORLD_COLOR,
                        );
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
        let origin_tick = pacing
            .pacer_origin_tick
            .map(|tick| tick.to_string())
            .unwrap_or_else(|| "--".to_owned());

        Self::draw_text_line(
            ctx,
            216.0,
            &format!(
                "AI OBSERVER: total {} | Candidate A {} | thread {} | players 0x{:X} | tick {} -> {} | player {}",
                pacing.total_think_calls,
                pacing.candidate_a_think_calls,
                pacing.last_candidate_a_thread,
                pacing.seen_player_mask,
                first_tick,
                last_tick,
                last_player,
            ),
            0x80d8ffff,
        );
        Self::draw_text_line(
            ctx,
            238.0,
            &format!(
                "PACER: origin tick {} | elapsed {} ms | manual finish {} | safety fail-open {} | waits {} | slept ~{} ms",
                origin_tick,
                pacing.pacer_elapsed_ms,
                if pacing.manual_finish_requested { "YES" } else { "no" },
                if pacing.safety_fail_open { "YES" } else { "no" },
                pacing.pacer_wait_count,
                pacing.pacer_total_wait_ms,
            ),
            0x80ffbfff,
        );

        let control_state = control::diagnostics();
        let selection = control_state
            .selected_player
            .map(|player| format!("F{} / player {}", player + 1, player))
            .unwrap_or_else(|| "none (press F1-F10)".to_owned());
        let target = control_state
            .move_target
            .map(|(x, y)| format!("({x},{y})"))
            .unwrap_or_else(|| "--".to_owned());
        let manual_tick = control_state
            .last_manual_tick
            .map(|tick| tick.to_string())
            .unwrap_or_else(|| "--".to_owned());
        Self::draw_text_line(
            ctx,
            260.0,
            &format!(
                "CONTROL: selected {} | target(sim) {} | selects {} | RMB cmds {} | manual returns {} | last tick {}",
                selection,
                target,
                control_state.select_count,
                control_state.move_command_count,
                control_state.manual_input_returns,
                manual_tick,
            ),
            if pacing_probe::manual_control_released() {
                0xff7070ff
            } else {
                0x80ffffff
            },
        );

        let projection_text = cursor_projection
            .map(|cursor| {
                format!(
                    "CURSOR: world ({:.2},{:.2}) -> sim ({},{})",
                    cursor.world_x, cursor.world_y, cursor.sim_x, cursor.sim_y
                )
            })
            .unwrap_or_else(|| "CURSOR: projection unavailable".to_owned());
        Self::draw_text_line(ctx, 282.0, &projection_text, 0x80d8ffff);
        Self::draw_text_line(
            ctx,
            304.0,
            "F1-F10: select raw player slot 0-9 (NO team/ownership policy) | RMB: force MoveTo cursor",
            0x80d8ffff,
        );
        Self::draw_text_line(
            ctx,
            326.0,
            "CTRL+END: release direct control and let simulation finish now — CANNOT RESUME THIS MATCH",
            if pacing.manual_finish_requested {
                0xff7070ff
            } else {
                0xffd080ff
            },
        );
        Self::draw_text_line(
            ctx,
            348.0,
            "TEST TARGET: marker + CURSOR sim coords appear; select any F1-F10; RMB increments cmds/returns and moves that slot.",
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
            control::reset();
            FINISH_CHORD_WAS_DOWN.store(false, Ordering::Release);
            SELECT_KEYS_WERE_DOWN.store(0, Ordering::Release);
            RMB_WAS_DOWN.store(false, Ordering::Release);
        }

        Self::poll_finish_chord(ingame);
        Self::poll_player_selection(ingame);

        if !Self::should_draw(ctx) {
            return;
        }

        let mouse = self.read_mouse(ctx);
        self.poll_rmb_move(ctx, mouse, ingame);

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
            "TFM2 Direct Control loaded (Stage 4B: paced Candidate A + team-neutral F1-F10 RMB MoveTo + Ctrl+End release)",
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
