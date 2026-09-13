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
const SKILL_YELLOW: u32 = 0xffd04070;
const SKILL_SKY_BLUE: u32 = 0x66ccff20;
const VK_F1_CODE: i32 = 0x70;
const PLAYER_SLOT_COUNT: usize = 10;
const SIM_UNITS_PER_WORLD_UNIT: f32 = 1000.0;

const SKILL_Q_KEY: &str = "Q";
const SKILL_W_KEY: &str = "W";
const SKILL_R_KEY: &str = "R";
const RETURN_HOME_KEY: &str = "B";
const HOLD_KEY: &str = "H";

static WAS_INGAME: AtomicBool = AtomicBool::new(false);
static START_CHORD_WAS_DOWN: AtomicBool = AtomicBool::new(false);
static FINISH_CHORD_WAS_DOWN: AtomicBool = AtomicBool::new(false);
static SELECT_KEYS_WERE_DOWN: AtomicU16 = AtomicU16::new(0);
static LMB_WAS_DOWN: AtomicBool = AtomicBool::new(false);
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
            (28.0, y, 1_120.0, 22.0),
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
        if let Some(athlete_id) = slot_mapping::resolve_fkey(ctx, slot) {
            control::select_athlete(athlete_id);
        }
    }

    fn poll_return_home(ctx: &StableClient<'_>, ingame: bool) {
        if !ingame
            || !pacing_probe::manual_input_enabled()
            || control::selected_athlete().is_none()
        {
            return;
        }

        if ctx.key_pressed(RETURN_HOME_KEY) {
            control::request_return_home();
        }
    }

    fn poll_hold(ctx: &StableClient<'_>, ingame: bool) {
        if !ingame
            || !pacing_probe::manual_input_enabled()
            || control::selected_athlete().is_none()
        {
            return;
        }

        if ctx.key_pressed(HOLD_KEY) {
            control::cancel_skill_targeting();
            control::clear_move_target();
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

        let (origin_ui_x, origin_ui_y) =
            if let Some((x, y, w, h)) = ctx.ui_node_rect("ingame.center_log") {
                if mouse.ui_x < x
                    || mouse.ui_y < y
                    || mouse.ui_x >= x + w
                    || mouse.ui_y >= y + h
                {
                    return None;
                }
                (x + w * 0.5, y + h * 0.5)
            } else {
                (ui_w * 0.5, ui_h * 0.5)
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
        })
    }

    fn best_camera() -> Option<camera_probe::CameraSnapshot> {
        camera_probe::snapshots()
            .iter()
            .max_by_key(|candidate| candidate.calls)
            .copied()
    }

    /// Returns true when a rising RMB was consumed as skill-target cancel.
    fn poll_skill_targeting(
        &self,
        ctx: &StableClient<'_>,
        mouse: MouseSnapshot,
        ingame: bool,
    ) -> bool {
        if !ingame {
            LMB_WAS_DOWN.store(false, Ordering::Release);
            return false;
        }

        let lmb_was_down = LMB_WAS_DOWN.swap(mouse.left_down, Ordering::AcqRel);
        let lmb_pressed = mouse.left_down && !lmb_was_down;
        let rmb_pressed = mouse.right_down && !RMB_WAS_DOWN.load(Ordering::Acquire);

        if !pacing_probe::manual_input_enabled() || control::selected_athlete().is_none() {
            return false;
        }

        if ctx.key_pressed("Escape") {
            control::cancel_skill_targeting();
            return false;
        }

        if ctx.key_pressed(SKILL_Q_KEY) {
            control::arm_skill(control::SkillSlot::Q);
        } else if ctx.key_pressed(SKILL_W_KEY) {
            control::arm_skill(control::SkillSlot::W);
        } else if ctx.key_pressed(SKILL_R_KEY) {
            control::arm_skill(control::SkillSlot::R);
        }

        if !control::skill_targeting_active() {
            control::clear_skill_cursor();
            return false;
        }

        if rmb_pressed {
            control::cancel_skill_targeting();
            return true;
        }

        let Some(camera) = Self::best_camera() else {
            control::clear_skill_cursor();
            return false;
        };
        let Some(cursor) = Self::cursor_world(ctx, mouse, camera) else {
            control::clear_skill_cursor();
            return false;
        };

        control::publish_skill_cursor(cursor.sim_x, cursor.sim_y);
        if lmb_pressed {
            control::confirm_skill(cursor.sim_x, cursor.sim_y);
        }
        false
    }

    fn poll_rmb_move(
        &self,
        ctx: &StableClient<'_>,
        mouse: MouseSnapshot,
        ingame: bool,
        suppress_rmb: bool,
    ) {
        if !ingame {
            RMB_WAS_DOWN.store(false, Ordering::Release);
            return;
        }

        let was_down = RMB_WAS_DOWN.swap(mouse.right_down, Ordering::AcqRel);
        if suppress_rmb
            || !mouse.right_down
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

    fn draw_skill_preview(ctx: &mut StableClient<'_>, camera: camera_probe::CameraSnapshot) {
        let skill = control::skill_targeting_snapshot();
        if skill.armed.is_none() {
            return;
        }
        let Some(self_sim) = skill.self_position else {
            return;
        };
        let Some(cursor_sim) = skill.cursor else {
            return;
        };

        let Some((game_w, game_h)) = ctx.draw_map_size("Game") else {
            return;
        };
        if game_w <= 0.0 || game_h <= 0.0 {
            return;
        }

        let units_per_px = ((camera.extent_a / game_w) + (camera.extent_b / game_h)) * 0.5;
        let self_world = (
            self_sim.0 as f32 / SIM_UNITS_PER_WORLD_UNIT,
            self_sim.1 as f32 / SIM_UNITS_PER_WORLD_UNIT,
        );

        ctx.draw_set_camera(
            "Game",
            camera.center_x,
            camera.center_y,
            camera.extent_a,
            camera.extent_b,
        );

        if skill.mode == control::SkillPreviewMode::None {
            ctx.draw_circle(
                "Game",
                self_world.0,
                self_world.1,
                13.0 * units_per_px,
                99_990,
                SKILL_YELLOW,
            );
            return;
        }

        let aim_sim = if let Some(range) = skill.range_sim {
            ctx.draw_circle(
                "Game",
                self_world.0,
                self_world.1,
                range as f32 / SIM_UNITS_PER_WORLD_UNIT,
                99_970,
                SKILL_SKY_BLUE,
            );
            control::clamp_skill_target_to_range(self_sim, cursor_sim, range)
        } else {
            cursor_sim
        };

        let aim_world = (
            aim_sim.0 as f32 / SIM_UNITS_PER_WORLD_UNIT,
            aim_sim.1 as f32 / SIM_UNITS_PER_WORLD_UNIT,
        );

        ctx.draw_line(
            "Game",
            self_world.0,
            self_world.1,
            aim_world.0,
            aim_world.1,
            5.0 * units_per_px,
            99_990,
            SKILL_YELLOW,
        );
        ctx.draw_circle(
            "Game",
            aim_world.0,
            aim_world.1,
            9.0 * units_per_px,
            99_991,
            SKILL_YELLOW,
        );
    }

    fn draw_cursor(ctx: &mut StableClient<'_>, mouse: MouseSnapshot) {
        if !mouse.valid {
            return;
        }
        let color = if mouse.right_down {
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
            color,
        );
        ctx.draw_line(
            "UI",
            mouse.ui_x,
            mouse.ui_y - 14.0,
            mouse.ui_x,
            mouse.ui_y + 14.0,
            2.0,
            20_000,
            color,
        );
        ctx.draw_circle("UI", mouse.ui_x, mouse.ui_y, 3.0, 20_001, color);
    }

    fn draw_world_cursor_and_skill(ctx: &mut StableClient<'_>, mouse: MouseSnapshot) {
        let Ok(()) = camera_probe::ensure_installed() else {
            return;
        };
        let Some(camera) = Self::best_camera() else {
            return;
        };

        if let Some(cursor) = Self::cursor_world(ctx, mouse, camera) {
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
        Self::draw_skill_preview(ctx, camera);
    }

    fn draw_start_gate(ctx: &mut StableClient<'_>) {
        ctx.draw_rect("UI", 18.0, 58.0, 820.0, 74.0, 19_998, 6.0, 0x101018dd);
        Self::draw_text_line(
            ctx,
            64.0,
            &format!(
                "DIRECT CONTROL: {} | Ctrl+Home starts live simulation",
                pacing_probe::presentation_phase_label()
            ),
            0x80ffbfff,
        );
        Self::draw_text_line(
            ctx,
            88.0,
            "Waiting for match view. Ctrl+End is the emergency permanent release.",
            0xffd080ff,
        );
    }

    fn draw_status(ctx: &mut StableClient<'_>, pause_ui: &pause_probe::PauseUiSnapshot) {
        let pacing = pacing_probe::snapshot();
        let control_state = control::diagnostics();
        let skill = control::skill_targeting_snapshot();

        let selected = control_state
            .selected_athlete
            .map(|id| id.to_string())
            .unwrap_or_else(|| "none".to_owned());
        let order = if control_state.returning {
            "return home".to_owned()
        } else if let Some(target_id) = control_state.attack_target {
            format!("attack {target_id}")
        } else if let Some((x, y)) = control_state.move_target {
            format!("move ({x},{y})")
        } else {
            "hold".to_owned()
        };
        let armed = skill.armed.map(|slot| slot.label()).unwrap_or("--");
        let range = skill
            .range_sim
            .map(|value| value.to_string())
            .unwrap_or_else(|| "?".to_owned());
        let pause_note = if pause_ui.paused {
            pause_ui.marker.as_deref().unwrap_or("presentation paused")
        } else {
            "running"
        };

        ctx.draw_rect("UI", 18.0, 58.0, 1_030.0, 112.0, 19_998, 6.0, 0x101018d8);
        Self::draw_text_line(
            ctx,
            62.0,
            &format!(
                "DIRECT CONTROL: {} | start {} | {}",
                pacing_probe::presentation_phase_label(),
                if pacing.start_requested { "YES" } else { "no" },
                pause_note
            ),
            if pause_ui.paused { 0xffd080ff } else { 0x80ffbfff },
        );
        Self::draw_text_line(
            ctx,
            84.0,
            &format!("SELECTED: athlete {selected} | ORDER: {order}"),
            0xffffffff,
        );
        Self::draw_text_line(
            ctx,
            106.0,
            &format!(
                "SKILL: {armed} | mode {} | range {range} | casts {} | rejected {}",
                skill.mode.label(),
                skill.cast_count,
                skill.reject_count
            ),
            if skill.armed.is_some() { 0xffd080ff } else { 0x80d8ffff },
        );
        Self::draw_text_line(
            ctx,
            128.0,
            "F1-F10 select | RMB move/attack | H hold | B return | Q/W/R arm | LMB confirm | RMB/Esc cancel | Ctrl+End release",
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
            pacing_probe::prepare_next_match();
            control::reset();
            slot_mapping::reset();
        }

        if ingame && !was_ingame {
            camera_probe::clear_candidates();
            pause_probe::reset();
            control::reset();
            slot_mapping::reset();
            START_CHORD_WAS_DOWN.store(false, Ordering::Release);
            FINISH_CHORD_WAS_DOWN.store(false, Ordering::Release);
            SELECT_KEYS_WERE_DOWN.store(0, Ordering::Release);
            LMB_WAS_DOWN.store(false, Ordering::Release);
            RMB_WAS_DOWN.store(false, Ordering::Release);
        }

        let pause_ui = pause_probe::update(ctx, ingame);
        Self::poll_start_chord(control_scene);
        pacing_probe::set_presentation_state(ingame, pause_ui.paused);
        Self::poll_finish_chord(ingame);
        Self::poll_player_selection(ctx, ingame);
        Self::poll_return_home(ctx, ingame);
        Self::poll_hold(ctx, ingame);

        if !control_scene {
            return;
        }

        let mouse = self.read_mouse(ctx);
        let skill_consumed_rmb = self.poll_skill_targeting(ctx, mouse, ingame);
        self.poll_rmb_move(ctx, mouse, ingame, skill_consumed_rmb);
        Self::draw_cursor(ctx, mouse);

        if ingame {
            Self::draw_world_cursor_and_skill(ctx, mouse);
            Self::draw_status(ctx, &pause_ui);
        } else {
            Self::draw_start_gate(ctx);
        }
    }
}

fn init(host: &StableHost) -> StableMod {
    match simulation_probe::ensure_installed() {
        Ok(()) => host.log(
            LogLevel::Info,
            "TFM2 Direct Control loaded (contextual RMB + manual skills + hold + return home)",
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
