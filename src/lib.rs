mod camera_probe;
mod control;
mod pacing_probe;
mod pause_probe;
mod simulation_probe;
mod slot_mapping;

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
            GetAsyncKeyState, VK_CONTROL, VK_END, VK_HOME, VK_LBUTTON, VK_RBUTTON,
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
static START_CHORD_WAS_DOWN: AtomicBool = AtomicBool::new(false);
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
    used_center_log_origin: bool,
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

    fn control_scene(ctx: &StableClient<'_>) -> bool {
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
            (28.0, y, 1_780.0, 22.0),
            19_999,
            13.0,
            color,
            TextAlignXV1::Left,
            TextAlignYV1::Center,
        );
    }

    fn poll_start_chord(control_scene: bool) {
        if !control_scene {
            START_CHORD_WAS_DOWN.store(false, Ordering::Release);
            return;
        }

        let chord_down = unsafe {
            GetAsyncKeyState(VK_CONTROL as i32) < 0 && GetAsyncKeyState(VK_HOME as i32) < 0
        };
        let was_down = START_CHORD_WAS_DOWN.swap(chord_down, Ordering::AcqRel);
        if chord_down && !was_down {
            pacing_probe::request_start_simulation();
        }
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

    fn poll_player_selection(ctx: &StableClient<'_>, ingame: bool) {
        if !ingame {
            SELECT_KEYS_WERE_DOWN.store(0, Ordering::Release);
            return;
        }

        let mut down_mask = 0u16;
        for slot in 0..PLAYER_SLOT_COUNT {
            let vk = VK_F1_CODE + slot as i32;
            if unsafe { GetAsyncKeyState(vk) } < 0 {
                down_mask |= 1u16 << slot;
            }
        }

        let previous = SELECT_KEYS_WERE_DOWN.swap(down_mask, Ordering::AcqRel);
        if !pacing_probe::manual_input_enabled() {
            return;
        }

        let rising = down_mask & !previous;
        if rising == 0 {
            return;
        }

        let slot = rising.trailing_zeros() as usize;
        // The visible F-key card order is not Candidate A's internal player_id order. Resolve the
        // displayed card to an athlete id and control by athlete identity instead. No team policy.
        if let Some(athlete_id) = slot_mapping::resolve_fkey(ctx, slot) {
            control::select_athlete(athlete_id);
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

        // This is the projection physically validated during the camera work. The cursor offset is
        // measured in logical UI coordinates but scaled by the backing Game render-map dimensions.
        // Do not normalize it through the 1920x1080 UI map first; that was the marker-offset regression.
        let (origin_ui_x, origin_ui_y, used_center_log_origin) =
            if let Some((x, y, w, h)) = ctx.ui_node_rect("ingame.center_log") {
                if mouse.ui_x < x
                    || mouse.ui_y < y
                    || mouse.ui_x >= x + w
                    || mouse.ui_y >= y + h
                {
                    return None;
                }
                (x + w * 0.5, y + h * 0.5, true)
            } else {
                (ui_w * 0.5, ui_h * 0.5, false)
            };

        let dx = mouse.ui_x - origin_ui_x;
        let dy = mouse.ui_y - origin_ui_y;
        let world_x = camera.center_x + dx * (camera.extent_a / game_w);
        let world_y = camera.center_y + dy * (camera.extent_b / game_h);
        if !world_x.is_finite() || !world_y.is_finite() || world_x < 0.0 || world_y < 0.0 {
            return None;
        }

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
            used_center_log_origin,
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
            || !pacing_probe::manual_input_enabled()
            || control::selected_athlete().is_none()
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

    fn draw_mouse_overlay(ctx: &mut StableClient<'_>, mouse: MouseSnapshot) {
        if !mouse.valid {
            return;
        }

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

    fn draw_start_gate(ctx: &mut StableClient<'_>) {
        let pacing = pacing_probe::snapshot();
        let first_tick = pacing
            .first_candidate_a_tick
            .map(|tick| tick.to_string())
            .unwrap_or_else(|| "--".to_owned());
        let last_tick = pacing
            .last_candidate_a_tick
            .map(|tick| tick.to_string())
            .unwrap_or_else(|| "--".to_owned());

        ctx.draw_rect("UI", 18.0, 58.0, 1_450.0, 92.0, 19_998, 6.0, 0x101018dd);
        Self::draw_text_line(
            ctx,
            62.0,
            &format!(
                "STAGE 5A START GATE: {} | Candidate A tick {} -> {} | held ~{} ms",
                pacing_probe::presentation_phase_label(),
                first_tick,
                last_tick,
                pacing.start_total_wait_ms,
            ),
            0x80ff9fff,
        );
        Self::draw_text_line(
            ctx,
            84.0,
            "CTRL+HOME: start/release Candidate A into 60 Hz live simulation",
            0xffd080ff,
        );
        Self::draw_text_line(
            ctx,
            106.0,
            "If the battlefield cannot appear while held, Ctrl+Home here is the escape hatch and proves loading depends on simulation progress.",
            0x80d8ffff,
        );
    }

    fn draw_probe(
        &self,
        ctx: &mut StableClient<'_>,
        mouse: MouseSnapshot,
        pause_ui: &pause_probe::PauseUiSnapshot,
    ) {
        ctx.draw_rect("UI", 18.0, 58.0, 1_820.0, 448.0, 19_998, 6.0, 0x101018dd);

        match simulation_probe::ensure_installed() {
            Ok(()) => Self::draw_text_line(
                ctx,
                62.0,
                "SIM TASK PROBE: A confirmed watched-match job | contextual RMB + authoritative manual control",
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
        let last_athlete = pacing
            .last_candidate_a_athlete
            .map(|athlete| athlete.to_string())
            .unwrap_or_else(|| "--".to_owned());
        let origin_tick = pacing
            .pacer_origin_tick
            .map(|tick| tick.to_string())
            .unwrap_or_else(|| "--".to_owned());

        Self::draw_text_line(
            ctx,
            216.0,
            &format!(
                "AI CALLBACKS (all sims) {} | Candidate A {} | thread {} | players 0x{:X} | tick {} -> {} | player {} athlete {}",
                pacing.total_think_calls,
                pacing.candidate_a_think_calls,
                pacing.last_candidate_a_thread,
                pacing.seen_player_mask,
                first_tick,
                last_tick,
                last_player,
                last_athlete,
            ),
            0x80d8ffff,
        );
        Self::draw_text_line(
            ctx,
            238.0,
            &format!(
                "PACER: phase {} | start {} | origin {} | elapsed {} ms | pace waits {} | start held ~{} ms | pause held ~{} ms",
                pacing_probe::presentation_phase_label(),
                if pacing.start_requested { "YES" } else { "no" },
                origin_tick,
                pacing.pacer_elapsed_ms,
                pacing.pacer_wait_count,
                pacing.start_total_wait_ms,
                pacing.pause_total_wait_ms,
            ),
            0x80ffbfff,
        );
        Self::draw_text_line(
            ctx,
            260.0,
            &format!(
                "JOB: entry {} ctx 0x{:X} | finish {} | fail-open {}",
                pacing.active_job_entry,
                pacing.active_job_context,
                if pacing.manual_finish_requested { "YES" } else { "no" },
                if pacing.safety_fail_open { "YES" } else { "no" },
            ),
            0x80ffbfff,
        );

        let pause_marker = pause_ui
            .marker
            .as_deref()
            .unwrap_or("pause_ui not visible");
        Self::draw_text_line(
            ctx,
            282.0,
            &format!(
                "PAUSE UI: detected {} | scanned {} nodes | {}",
                if pause_ui.paused { "YES" } else { "no" },
                pause_ui.scanned_nodes,
                pause_marker,
            ),
            if pause_ui.paused { 0xffd080ff } else { 0x80ffffff },
        );

        let mapping = slot_mapping::snapshot();
        let mapping_text = if let Some(error) = mapping.error.as_deref() {
            format!(
                "F-KEY MAP: F{} FAILED: {} | card {:?}",
                mapping.fkey_slot.map(|slot| slot + 1).unwrap_or(0),
                error,
                mapping.card_text,
            )
        } else if let (Some(slot), Some(name), Some(athlete_id)) =
            (mapping.fkey_slot, mapping.athlete_name.as_deref(), mapping.athlete_id)
        {
            format!(
                "F-KEY MAP: F{} -> {} -> athlete {} | card {:?}",
                slot + 1,
                name,
                athlete_id,
                mapping.card_text,
            )
        } else {
            "F-KEY MAP: -- (press F1-F10 after starting)".to_owned()
        };
        Self::draw_text_line(ctx, 304.0, &mapping_text, 0x80d8ffff);

        let control_state = control::diagnostics();
        let selection = control_state
            .selected_athlete
            .map(|athlete| format!("athlete {athlete}"))
            .unwrap_or_else(|| "none".to_owned());
        let target = control_state
            .move_target
            .map(|(x, y)| format!("({x},{y})"))
            .unwrap_or_else(|| "--".to_owned());
        let attack = control_state
            .attack_target
            .map(|target_id| target_id.to_string())
            .unwrap_or_else(|| "--".to_owned());
        let manual_tick = control_state
            .last_manual_tick
            .map(|tick| tick.to_string())
            .unwrap_or_else(|| "--".to_owned());
        Self::draw_text_line(
            ctx,
            326.0,
            &format!(
                "CONTROL: selected {} | RMB(sim) {} | attack {} | cmds {} | resolved M{} A{} | returns {} (atk {}) | tick {}",
                selection,
                target,
                attack,
                control_state.move_command_count,
                control_state.move_resolve_count,
                control_state.attack_resolve_count,
                control_state.manual_input_returns,
                control_state.attack_input_returns,
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
                    "CURSOR: world ({:.2},{:.2}) -> sim ({},{}) | origin {}",
                    cursor.world_x,
                    cursor.world_y,
                    cursor.sim_x,
                    cursor.sim_y,
                    if cursor.used_center_log_origin { "center_log" } else { "UI center fallback" },
                )
            })
            .unwrap_or_else(|| "CURSOR: projection unavailable".to_owned());
        Self::draw_text_line(ctx, 348.0, &projection_text, 0x80d8ffff);
        Self::draw_text_line(
            ctx,
            370.0,
            "CTRL+HOME = START | F1-F10 = select athlete | RMB ground = move | RMB hostile = exact attack",
            if pacing.start_requested { 0x80d8ffff } else { 0xffd080ff },
        );
        Self::draw_text_line(
            ctx,
            392.0,
            "CTRL+END = release control + pacing and finish simulation — CANNOT RESUME THIS MATCH",
            if pacing.manual_finish_requested { 0xff7070ff } else { 0xffd080ff },
        );
        Self::draw_text_line(
            ctx,
            414.0,
            "TEST: selected athlete stays manual even after target loss; yellow marker should remain under reticle while pan/zoom/layout change.",
            0x80d8ffff,
        );
    }
}

impl StableExtension for DirectControlExtension {
    fn post_render(&self, ctx: &mut StableClient<'_>) {
        let ingame = matches!(ctx.client_scene_kind(), Some(ClientSceneKindV1::InGame));
        let control_scene = Self::control_scene(ctx);
        let was_ingame = WAS_INGAME.swap(ingame, Ordering::AcqRel);

        if !ingame && was_ingame {
            // Prepare the one-way start/release state for the next match. The actual Candidate-A
            // job boundary is also detected on the simulation thread.
            pacing_probe::prepare_next_match();
            control::reset();
            slot_mapping::reset();
        }

        if ingame && !was_ingame {
            camera_probe::clear_candidates();
            // Deliberately DO NOT reset pacing here. Resetting the pacer at InGame was what erased
            // the relationship to pre-match simulation time and created hidden lead.
            pause_probe::reset();
            control::reset();
            slot_mapping::reset();
            START_CHORD_WAS_DOWN.store(false, Ordering::Release);
            FINISH_CHORD_WAS_DOWN.store(false, Ordering::Release);
            SELECT_KEYS_WERE_DOWN.store(0, Ordering::Release);
            RMB_WAS_DOWN.store(false, Ordering::Release);
        }

        let pause_ui = pause_probe::update(ctx, ingame);
        Self::poll_start_chord(control_scene);
        pacing_probe::set_presentation_state(ingame, pause_ui.paused);
        Self::poll_finish_chord(ingame);
        Self::poll_player_selection(ctx, ingame);

        if !control_scene {
            return;
        }

        let mouse = self.read_mouse(ctx);
        self.poll_rmb_move(ctx, mouse, ingame);
        Self::draw_mouse_overlay(ctx, mouse);

        if ingame {
            self.draw_probe(ctx, mouse, &pause_ui);
        } else {
            Self::draw_start_gate(ctx);
        }
    }
}

fn init(host: &StableHost) -> StableMod {
    match simulation_probe::ensure_installed() {
        Ok(()) => host.log(
            LogLevel::Info,
            "TFM2 Direct Control loaded (contextual RMB + authoritative manual control)",
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
