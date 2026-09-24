mod camera_probe;
mod control;
mod input_focus;
mod minimap;
mod pacing_probe;
mod pause_probe;
mod simulation_probe;
mod slot_mapping;

use std::sync::{
    atomic::{AtomicBool, AtomicU16, Ordering},
    Mutex,
};

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
const SKILL_YELLOW: u32 = 0xffd04070;
const SKILL_SKY_BLUE: u32 = 0x66ccff20;
const PICK_OVERLAY_ENEMY_CHAMPION: u32 = 0xe04848c0;
const PICK_OVERLAY_ALLY_CHAMPION: u32 = 0x78f090c0;
const PICK_OVERLAY_ENEMY: u32 = 0x8f202080;
const PICK_OVERLAY_ALLY: u32 = 0x48b86080;
const PICK_OVERLAY_ENEMY_CREEP: u32 = 0x8f20204c;
const PICK_OVERLAY_ALLY_CREEP: u32 = 0x48b8604c;
const VK_F1_CODE: i32 = 0x70;
const PLAYER_SLOT_COUNT: usize = 10;
const SIM_UNITS_PER_WORLD_UNIT: f32 = 1000.0;
const MAP_WORLD_MIN: f32 = 0.0;
const MAP_WORLD_MAX: f32 = 960.0;

const ATTACK_MOVE_KEY: &str = "A";
const SKILL_Q_KEY: &str = "Q";
const SKILL_W_KEY: &str = "W";
const SKILL_R_KEY: &str = "R";
const RETURN_HOME_KEY: &str = "B";
const HOLD_KEY: &str = "H";

static MATCH_SESSION_ACTIVE: AtomicBool = AtomicBool::new(false);
static START_CHORD_WAS_DOWN: AtomicBool = AtomicBool::new(false);
static FINISH_CHORD_WAS_DOWN: AtomicBool = AtomicBool::new(false);
static FINISH_CONFIRM_ACTIVE: AtomicBool = AtomicBool::new(false);
static FINISH_CONFIRM_LMB_WAS_DOWN: AtomicBool = AtomicBool::new(false);
static STARTUP_SPEED_OVERRIDE_ACTIVE: AtomicBool = AtomicBool::new(false);
static TEMP_RELEASE_WAS_DOWN: AtomicBool = AtomicBool::new(false);
static SELECT_KEYS_WERE_DOWN: AtomicU16 = AtomicU16::new(0);
static LMB_WAS_DOWN: AtomicBool = AtomicBool::new(false);
static RMB_WAS_DOWN: AtomicBool = AtomicBool::new(false);
static NATIVE_SEEK_CONTROLS_SUPPRESSED: AtomicBool = AtomicBool::new(false);
static NATIVE_SEEK_CONTROL_NODES: Mutex<Vec<(String, bool)>> = Mutex::new(Vec::new());

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
    sim_x: u64,
    sim_y: u64,
    sim_units_per_px: u64,
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
        if !input_focus::process_owns_foreground_window() {
            START_CHORD_WAS_DOWN.store(true, Ordering::Release);
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
            FINISH_CONFIRM_ACTIVE.store(false, Ordering::Release);
            FINISH_CONFIRM_LMB_WAS_DOWN.store(false, Ordering::Release);
            return;
        }
        if !input_focus::process_owns_foreground_window() {
            FINISH_CHORD_WAS_DOWN.store(true, Ordering::Release);
            return;
        }

        let chord_down = unsafe {
            GetAsyncKeyState(VK_CONTROL as i32) < 0 && GetAsyncKeyState(VK_END as i32) < 0
        };
        let was_down = FINISH_CHORD_WAS_DOWN.swap(chord_down, Ordering::AcqRel);
        if chord_down && !was_down && !pacing_probe::manual_control_released() {
            FINISH_CONFIRM_ACTIVE.store(true, Ordering::Release);
            FINISH_CONFIRM_LMB_WAS_DOWN.store(false, Ordering::Release);
        }
    }

    fn poll_finish_confirmation(mouse: MouseSnapshot) {
        if !FINISH_CONFIRM_ACTIVE.load(Ordering::Acquire) {
            FINISH_CONFIRM_LMB_WAS_DOWN.store(false, Ordering::Release);
            return;
        }

        let was_down = FINISH_CONFIRM_LMB_WAS_DOWN.swap(mouse.left_down, Ordering::AcqRel);
        if !mouse.valid || !mouse.left_down || was_down {
            return;
        }

        let yes = mouse.ui_x >= 760.0
            && mouse.ui_x <= 930.0
            && mouse.ui_y >= 590.0
            && mouse.ui_y <= 646.0;
        let no = mouse.ui_x >= 990.0
            && mouse.ui_x <= 1_160.0
            && mouse.ui_y >= 590.0
            && mouse.ui_y <= 646.0;

        if yes {
            FINISH_CONFIRM_ACTIVE.store(false, Ordering::Release);
            pacing_probe::request_finish_simulation();
        } else if no {
            FINISH_CONFIRM_ACTIVE.store(false, Ordering::Release);
        }
    }

    fn poll_temporary_release(ingame: bool) {
        if !ingame {
            TEMP_RELEASE_WAS_DOWN.store(false, Ordering::Release);
            return;
        }
        if !input_focus::process_owns_foreground_window() {
            TEMP_RELEASE_WAS_DOWN.store(true, Ordering::Release);
            return;
        }

        let end_down = unsafe { GetAsyncKeyState(VK_END as i32) < 0 };
        let ctrl_down = unsafe { GetAsyncKeyState(VK_CONTROL as i32) < 0 };
        let was_down = TEMP_RELEASE_WAS_DOWN.swap(end_down, Ordering::AcqRel);

        if end_down
            && !was_down
            && !ctrl_down
            && !pacing_probe::manual_control_released()
            && control::selected_athlete().is_some()
        {
            // End relinquishes only our selected/manual champion. `control::reset()` clears retained
            // orders and targeting, but deliberately does not touch pacing or the Candidate-A job.
            // On the next simulation callback the athlete therefore receives its normal base_input.
            control::reset();
        }
    }

    fn poll_player_selection(ctx: &StableClient<'_>, ingame: bool) {
        if !ingame {
            SELECT_KEYS_WERE_DOWN.store(0, Ordering::Release);
            return;
        }
        if !input_focus::process_owns_foreground_window() {
            SELECT_KEYS_WERE_DOWN.store((1u16 << PLAYER_SLOT_COUNT) - 1, Ordering::Release);
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
            control::request_hold();
        }
    }

    fn cursor_world_impl(
        ctx: &StableClient<'_>,
        mouse: MouseSnapshot,
        camera: camera_probe::CameraSnapshot,
        clamp_to_map: bool,
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
        let raw_world_x = camera.center_x + dx * (camera.extent_a / game_w);
        let raw_world_y = camera.center_y + dy * (camera.extent_b / game_h);
        if !raw_world_x.is_finite() || !raw_world_y.is_finite() {
            return None;
        }

        let (world_x, world_y) = if clamp_to_map {
            (
                raw_world_x.clamp(MAP_WORLD_MIN, MAP_WORLD_MAX),
                raw_world_y.clamp(MAP_WORLD_MIN, MAP_WORLD_MAX),
            )
        } else {
            if raw_world_x < 0.0 || raw_world_y < 0.0 {
                return None;
            }
            (raw_world_x, raw_world_y)
        };

        let sim_x_f = world_x * SIM_UNITS_PER_WORLD_UNIT;
        let sim_y_f = world_y * SIM_UNITS_PER_WORLD_UNIT;
        if !sim_x_f.is_finite() || !sim_y_f.is_finite() || sim_x_f < 0.0 || sim_y_f < 0.0 {
            return None;
        }

        let world_units_per_px =
            ((camera.extent_a / game_w) + (camera.extent_b / game_h)) * 0.5;
        let sim_units_per_px_f = world_units_per_px * SIM_UNITS_PER_WORLD_UNIT;
        let sim_units_per_px = if sim_units_per_px_f.is_finite() && sim_units_per_px_f > 0.0 {
            sim_units_per_px_f.round() as u64
        } else {
            0
        };

        Some(CursorWorld {
            sim_x: sim_x_f.round() as u64,
            sim_y: sim_y_f.round() as u64,
            sim_units_per_px,
        })
    }

    fn cursor_world(
        ctx: &StableClient<'_>,
        mouse: MouseSnapshot,
        camera: camera_probe::CameraSnapshot,
    ) -> Option<CursorWorld> {
        Self::cursor_world_impl(ctx, mouse, camera, false)
    }

    fn movement_cursor_world(
        ctx: &StableClient<'_>,
        mouse: MouseSnapshot,
        camera: camera_probe::CameraSnapshot,
    ) -> Option<CursorWorld> {
        // An off-map movement click expresses direction, not permission to leave the map.
        // Clamp only the requested destination into the legal 0..960 world square. TFM2 still
        // receives its ordinary MoveTo/attack-move request, so native pathing, terrain, entity
        // collision, and champion radius remain authoritative.
        Self::cursor_world_impl(ctx, mouse, camera, true)
    }

    fn best_camera() -> Option<camera_probe::CameraSnapshot> {
        camera_probe::snapshots()
            .iter()
            .max_by_key(|candidate| candidate.calls)
            .copied()
    }

    /// Polls the shared LMB/RMB targeting layer for A attack-move and Q/W/R skill targeting.
    /// Returns true only when a rising RMB was consumed purely as a skill-target cancel.
    fn poll_targeting(
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
            control::cancel_attack_move();
            control::cancel_skill_targeting();
            return false;
        }

        if ctx.key_pressed(ATTACK_MOVE_KEY) {
            control::arm_attack_move();
        } else if ctx.key_pressed(SKILL_Q_KEY) {
            control::arm_skill(control::SkillSlot::Q);
        } else if ctx.key_pressed(SKILL_W_KEY) {
            control::arm_skill(control::SkillSlot::W);
        } else if ctx.key_pressed(SKILL_R_KEY) {
            control::arm_skill(control::SkillSlot::R);
        }

        if control::attack_move_armed() {
            // RMB cancels the A cursor but is deliberately *not* consumed: the normal contextual RMB
            // path below is allowed to replace the current order, matching ordinary MOBA expectations.
            if rmb_pressed {
                control::cancel_attack_move();
                return false;
            }

            let Some(camera) = Self::best_camera() else {
                return false;
            };
            let Some(cursor) = Self::movement_cursor_world(ctx, mouse, camera) else {
                return false;
            };

            if lmb_pressed {
                control::confirm_attack_move(cursor.sim_x, cursor.sim_y);
            }
            return false;
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

        // Keep edge state current for the shared targeting layer, but unlike the original
        // click-only implementation do not require a rising edge here. While RMB remains held,
        // republish the current cursor every render pass. Candidate A consumes only the latest
        // coherent request on its next simulation tick and performs the same authoritative
        // contextual Attack-vs-Move resolution it already uses for single clicks.
        RMB_WAS_DOWN.store(mouse.right_down, Ordering::Release);
        if suppress_rmb
            || !mouse.right_down
            || !mouse.valid
            || !pacing_probe::manual_input_enabled()
            || control::selected_athlete().is_none()
        {
            return;
        }

        // Minimap sits outside ingame.center_log, so resolve it before the camera/world projection.
        // The live UI tree supplies its current rectangle; no resolution-specific box is hard-coded.
        if let Some((sim_x, sim_y)) = minimap::cursor_to_sim(ctx, mouse.ui_x, mouse.ui_y) {
            control::publish_move_target(sim_x, sim_y);
            return;
        }

        let Some(camera) = Self::best_camera() else {
            return;
        };
        let Some(cursor) = Self::movement_cursor_world(ctx, mouse, camera) else {
            return;
        };

        control::publish_move_target_with_pick_scale(
            cursor.sim_x,
            cursor.sim_y,
            cursor.sim_units_per_px,
        );
    }


    fn draw_click_target_overlays(
        ctx: &mut StableClient<'_>,
        camera: camera_probe::CameraSnapshot,
    ) {
        if !pacing_probe::manual_input_enabled() || control::selected_athlete().is_none() {
            return;
        }
        let Some(controlled_team) = control::selected_team() else {
            return;
        };
        let Some((game_w, game_h)) = ctx.draw_map_size("Game") else {
            return;
        };
        let (ui_w, ui_h) = ctx
            .draw_map_size("UI")
            .unwrap_or((UI_FALLBACK_W, UI_FALLBACK_H));
        if game_w <= 0.0
            || game_h <= 0.0
            || ui_w <= 0.0
            || ui_h <= 0.0
            || camera.extent_a <= 0.0
            || camera.extent_b <= 0.0
        {
            return;
        }

        let (origin_ui_x, origin_ui_y) =
            if let Some((x, y, w, h)) = ctx.ui_node_rect("ingame.center_log") {
                (x + w * 0.5, y + h * 0.5)
            } else {
                (ui_w * 0.5, ui_h * 0.5)
            };

        let world_units_per_px =
            ((camera.extent_a / game_w) + (camera.extent_b / game_h)) * 0.5;
        let sim_units_per_px_f = world_units_per_px * SIM_UNITS_PER_WORLD_UNIT;
        if !sim_units_per_px_f.is_finite() || sim_units_per_px_f <= 0.0 {
            return;
        }
        let sim_units_per_px = sim_units_per_px_f.round() as u64;
        let ui_per_world_x = game_w / camera.extent_a;
        let ui_per_world_y = game_h / camera.extent_b;

        for entity in control::click_target_overlay_snapshot() {
            let radius_sim = control::click_target_effective_radius(
                entity.kind,
                entity.collision_radius,
                sim_units_per_px,
            );
            if radius_sim == 0 {
                continue;
            }

            let friendly = entity.team == controlled_team;
            let color = match entity.kind {
                control::EntityKind::Champion if friendly => PICK_OVERLAY_ALLY_CHAMPION,
                control::EntityKind::Champion => PICK_OVERLAY_ENEMY_CHAMPION,
                control::EntityKind::Minion if friendly => PICK_OVERLAY_ALLY_CREEP,
                control::EntityKind::Minion => PICK_OVERLAY_ENEMY_CREEP,
                _ if friendly => PICK_OVERLAY_ALLY,
                _ => PICK_OVERLAY_ENEMY,
            };

            let world_x = entity.x as f32 / SIM_UNITS_PER_WORLD_UNIT;
            let world_y = entity.y as f32 / SIM_UNITS_PER_WORLD_UNIT;
            let world_radius = radius_sim as f32 / SIM_UNITS_PER_WORLD_UNIT;
            let center_x = origin_ui_x + (world_x - camera.center_x) * ui_per_world_x;
            let center_y = origin_ui_y + (world_y - camera.center_y) * ui_per_world_y;
            let radius_x = world_radius * ui_per_world_x;
            let radius_y = world_radius * ui_per_world_y;

            // Do not spend draw calls on hitboxes wholly outside the visible viewport.
            if center_x + radius_x < 0.0
                || center_x - radius_x > ui_w
                || center_y + radius_y < 0.0
                || center_y - radius_y > ui_h
            {
                continue;
            }

            let (segments, line_width_px) = match entity.kind {
                control::EntityKind::Champion => (16usize, 3.0),
                control::EntityKind::Minion => (8usize, 1.0),
                _ => (12usize, 2.0),
            };

            let step = std::f32::consts::TAU / segments as f32;
            let mut previous_x = center_x + radius_x;
            let mut previous_y = center_y;
            for segment in 1..=segments {
                let angle = segment as f32 * step;
                let next_x = center_x + radius_x * angle.cos();
                let next_y = center_y + radius_y * angle.sin();
                ctx.draw_line(
                    "UI",
                    previous_x,
                    previous_y,
                    next_x,
                    next_y,
                    line_width_px,
                    19_990,
                    color,
                );
                previous_x = next_x;
                previous_y = next_y;
            }
        }
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

    fn draw_targeting_overlays(ctx: &mut StableClient<'_>) {
        let Ok(()) = camera_probe::ensure_installed() else {
            return;
        };
        let Some(camera) = Self::best_camera() else {
            return;
        };

        Self::draw_click_target_overlays(ctx, camera);
        Self::draw_skill_preview(ctx, camera);
    }

    fn draw_start_gate(ctx: &mut StableClient<'_>) {
        ctx.draw_rect("UI", 18.0, 58.0, 430.0, 34.0, 19_998, 6.0, 0x101018dd);
        Self::draw_text_line(
            ctx,
            64.0,
            &format!(
                "DIRECT CONTROL: {}",
                pacing_probe::presentation_phase_label()
            ),
            0x80ffbfff,
        );
    }

    fn native_seek_control_candidates(ctx: &StableClient<'_>) -> Vec<(String, bool)> {
        const MAX_DEPTH: usize = 6;
        const MAX_NODES: usize = 768;

        let mut found = Vec::new();
        let mut pending = vec![("ingame".to_owned(), 0usize)];
        let mut visited = 0usize;

        while let Some((parent, depth)) = pending.pop() {
            if depth >= MAX_DEPTH || visited >= MAX_NODES {
                continue;
            }

            for child in ctx.ui_child_names(&parent) {
                if visited >= MAX_NODES {
                    break;
                }
                visited += 1;

                let path = format!("{parent}.{child}");
                if depth + 1 < MAX_DEPTH {
                    pending.push((path.clone(), depth + 1));
                }

                let runner = ctx.ui_runner_name(&path).unwrap_or_default();
                if !(runner.contains("button") || runner.contains("selectable")) {
                    continue;
                }

                let Some((x, y, w, h)) = ctx.ui_node_rect(&path) else {
                    continue;
                };
                let center_x = x + w * 0.5;
                let center_y = y + h * 0.5;

                // TFM2's native replay seek/highlight toolbar sits directly under the blue-team
                // header in the upper-left of the 1920x1080 UI map. Restrict discovery to small
                // interactive widgets in that strip so the rest of the match UI remains untouched.
                if (0.0..=430.0).contains(&center_x)
                    && (58.0..=132.0).contains(&center_y)
                    && w <= 90.0
                    && h <= 90.0
                {
                    let was_visible = ctx.ui_visible(&path).unwrap_or(true);
                    found.push((path, was_visible));
                }
            }
        }

        found
    }

    fn update_native_seek_controls(ctx: &mut StableClient<'_>) {
        let scene = ctx.client_scene_kind();
        let live_session = matches!(scene, Some(ClientSceneKindV1::InGame))
            || (MATCH_SESSION_ACTIVE.load(Ordering::Acquire) && Self::control_scene(ctx));
        let should_suppress = live_session && !pacing_probe::manual_control_released();

        if should_suppress {
            if !NATIVE_SEEK_CONTROLS_SUPPRESSED.load(Ordering::Acquire) {
                let nodes = Self::native_seek_control_candidates(ctx);
                if nodes.is_empty() {
                    return;
                }

                for (path, _) in &nodes {
                    let _ = ctx.ui_set_visible(path, false);
                }

                if let Ok(mut cached) = NATIVE_SEEK_CONTROL_NODES.lock() {
                    *cached = nodes;
                    NATIVE_SEEK_CONTROLS_SUPPRESSED.store(true, Ordering::Release);
                }
            } else if let Ok(cached) = NATIVE_SEEK_CONTROL_NODES.lock() {
                // Reassert suppression in case the match UI rebuilt a runner while paused.
                for (path, _) in cached.iter() {
                    let _ = ctx.ui_set_visible(path, false);
                }
            }
            return;
        }

        if !NATIVE_SEEK_CONTROLS_SUPPRESSED.swap(false, Ordering::AcqRel) {
            return;
        }

        if let Ok(mut cached) = NATIVE_SEEK_CONTROL_NODES.lock() {
            for (path, was_visible) in cached.iter() {
                let _ = ctx.ui_set_visible(path, *was_visible);
            }
            cached.clear();
        }
    }

    fn visible_match_seconds(ctx: &StableClient<'_>) -> Option<u64> {
        let text = ctx
            .ui_text("ingame.header.game_time.value")
            .or_else(|| ctx.ui_text("header.game_time.value"))?;
        let mut total = 0u64;
        for part in text.trim().split(':') {
            let value = part.trim().parse::<u64>().ok()?;
            total = total.checked_mul(60)?.checked_add(value)?;
        }
        Some(total)
    }

    fn set_native_speed_selected(ctx: &mut StableClient<'_>, selected_path: &str) -> bool {
        const SPEED_PATHS: [&str; 5] = [
            "ingame.speed_buttons.speed05x",
            "ingame.speed_buttons.speed1x",
            "ingame.speed_buttons.speed15x",
            "ingame.speed_buttons.speed2x",
            "ingame.speed_buttons.speed3x",
        ];

        let mut recognized = 0usize;
        let mut writes_ok = true;
        for path in SPEED_PATHS {
            if ctx.ui_selectable_selected(path).is_none() {
                continue;
            }
            recognized += 1;
            let selected = path == selected_path;
            writes_ok &= ctx.ui_set_selectable_selected(path, selected);
        }

        recognized > 0
            && writes_ok
            && matches!(ctx.ui_selectable_selected(selected_path), Some(true))
    }

    fn update_startup_presentation_sync(ctx: &mut StableClient<'_>) {
        if !matches!(ctx.client_scene_kind(), Some(ClientSceneKindV1::InGame))
            || pacing_probe::start_requested()
            || pacing_probe::manual_control_released()
        {
            return;
        }

        let Some(ready_tick) = pacing_probe::ready_gate_tick() else {
            return;
        };
        let Some(visible_seconds) = Self::visible_match_seconds(ctx) else {
            pacing_probe::set_startup_presentation_synced(false);
            return;
        };


        // The visible clock is whole-second precision. Reaching floor(live_tick / 60) proves the
        // viewer has reached the frozen live second. A sub-second residual cannot be observed through
        // the stable client API and is deferred to the post-release playback-position watchdog.
        let target_seconds = ready_tick / 60;
        if visible_seconds < target_seconds {
            let ok = Self::set_native_speed_selected(ctx, "ingame.speed_buttons.speed3x");
            if ok {
                STARTUP_SPEED_OVERRIDE_ACTIVE.store(true, Ordering::Release);
            }
            pacing_probe::set_startup_presentation_synced(false);
        } else {
            let override_active = STARTUP_SPEED_OVERRIDE_ACTIVE.load(Ordering::Acquire);
            if override_active {
                let restored = Self::set_native_speed_selected(ctx, "ingame.speed_buttons.speed1x");
                if restored {
                    STARTUP_SPEED_OVERRIDE_ACTIVE.store(false, Ordering::Release);
                    pacing_probe::set_startup_presentation_synced(true);
                } else {
                    // We successfully changed the native selection earlier, so fail closed until
                    // 1x can be restored; never hand control over while a forced fast speed remains.
                    pacing_probe::set_startup_presentation_synced(false);
                }
            } else {
                // If programmatic speed selection is unsupported, do not deadlock startup. The
                // frozen simulation lets ordinary 1x presentation catch up safely on its own.
                pacing_probe::set_startup_presentation_synced(true);
            }
        }
    }

    fn draw_ready_prompt(ctx: &mut StableClient<'_>) {
        let synced = pacing_probe::startup_presentation_synced();
        let message = if synced {
            "Direct Control is ready. Press Ctrl+Home to take control and resume the match."
        } else {
            "Synchronizing Direct Control with the live match..."
        };

        ctx.draw_rect("UI", 520.0, 160.0, 880.0, 86.0, 30_000, 10.0, 0x101018e8);
        ctx.draw_text(
            "UI",
            message,
            "asset/base/font/set/bold",
            (550.0, 176.0, 820.0, 54.0),
            30_001,
            24.0,
            0xffffffff,
            TextAlignXV1::Center,
            TextAlignYV1::Center,
        );
    }

    fn draw_finish_confirmation(ctx: &mut StableClient<'_>) {
        ctx.draw_rect("UI", 610.0, 405.0, 700.0, 290.0, 40_000, 14.0, 0x101018f4);
        ctx.draw_text(
            "UI",
            "Give control back to the AI?",
            "asset/base/font/set/bold",
            (650.0, 438.0, 620.0, 46.0),
            40_001,
            28.0,
            0xffffffff,
            TextAlignXV1::Center,
            TextAlignYV1::Center,
        );
        ctx.draw_text(
            "UI",
            "Ctrl+End permanently releases Direct Control for this match. You cannot take control again until the next match.",
            "asset/base/font/set/regular",
            (690.0, 495.0, 540.0, 66.0),
            40_001,
            17.0,
            0xd8d8e8ff,
            TextAlignXV1::Center,
            TextAlignYV1::Center,
        );

        ctx.draw_rect("UI", 760.0, 590.0, 170.0, 56.0, 40_001, 8.0, 0x397a4fff);
        ctx.draw_text(
            "UI",
            "YES — RELEASE",
            "asset/base/font/set/bold",
            (760.0, 590.0, 170.0, 56.0),
            40_002,
            17.0,
            0xffffffff,
            TextAlignXV1::Center,
            TextAlignYV1::Center,
        );

        ctx.draw_rect("UI", 990.0, 590.0, 170.0, 56.0, 40_001, 8.0, 0x633b47ff);
        ctx.draw_text(
            "UI",
            "NO — KEEP CONTROL",
            "asset/base/font/set/bold",
            (990.0, 590.0, 170.0, 56.0),
            40_002,
            16.0,
            0xffffffff,
            TextAlignXV1::Center,
            TextAlignYV1::Center,
        );
    }

    fn draw_status(ctx: &mut StableClient<'_>, pause_ui: &pause_probe::PauseUiSnapshot) {
        let control_state = control::diagnostics();
        let skill = control::skill_targeting_snapshot();

        let controlled = control_state
            .selected_athlete
            .and_then(|id| ctx.athlete_name(id))
            .unwrap_or_else(|| "spectator".to_owned());

        let order = if control_state.selected_athlete.is_none() {
            "AI"
        } else if control_state.returning {
            "return home"
        } else if control_state.attack_moving {
            "attack-move"
        } else if control_state.attack_target.is_some() {
            "attack"
        } else if control_state.move_target.is_some() {
            "move"
        } else {
            "hold"
        };

        let targeting = if control::attack_move_armed() {
            Some("A-MOVE — LMB confirm | RMB/Esc cancel".to_owned())
        } else {
            skill.armed.map(|slot| {
                format!(
                    "{} {} — LMB confirm | RMB/Esc cancel",
                    slot.label(),
                    skill.mode.label()
                )
            })
        };

        let status = if control_state.selected_athlete.is_none() {
            format!(
                "DIRECT CONTROL: {} | spectator",
                pacing_probe::presentation_phase_label()
            )
        } else {
            format!(
                "DIRECT CONTROL: {} | {controlled} | {order}",
                pacing_probe::presentation_phase_label()
            )
        };

        let height = if targeting.is_some() { 56.0 } else { 34.0 };
        ctx.draw_rect("UI", 18.0, 58.0, 720.0, height, 19_998, 6.0, 0x101018d8);
        Self::draw_text_line(
            ctx,
            64.0,
            &status,
            if pause_ui.paused { 0xffd080ff } else { 0x80ffbfff },
        );

        if let Some(targeting) = targeting {
            Self::draw_text_line(ctx, 86.0, &targeting, 0xffffffff);
        }
    }
}

impl StableExtension for DirectControlExtension {
    fn post_update(&self, ctx: &mut StableClient<'_>, _dt_micros: u64) {
        Self::update_startup_presentation_sync(ctx);
        Self::update_native_seek_controls(ctx);
    }

    fn post_render(&self, ctx: &mut StableClient<'_>) {
        let scene = ctx.client_scene_kind();
        let ingame = matches!(scene, Some(ClientSceneKindV1::InGame));
        let control_scene = Self::control_scene(ctx);
        let session_was_active = MATCH_SESSION_ACTIVE.load(Ordering::Acquire);

        if matches!(scene, Some(ClientSceneKindV1::Match)) {
            pacing_probe::note_match_render();
        }
        if ctx.draw_map_size("Game").is_some() {
            pacing_probe::note_game_map_ready();
        }
        if ctx.ui_node_rect("ingame.center_log").is_some() {
            pacing_probe::note_center_log_ready();
        }
        if ingame {
            pacing_probe::note_ingame_render();
        }

        // Match and InGame are both part of the live match-view lifecycle. Do not treat a
        // temporary transition from InGame -> Match (for example a pause/menu presentation state)
        // as the end of the simulation session. That used to reset manual control and could let
        // vanilla AI finish the watched simulation while the user was merely paused.
        let session_active = if ingame {
            if !session_was_active {
                MATCH_SESSION_ACTIVE.store(true, Ordering::Release);
                camera_probe::clear_candidates();
                pause_probe::reset();
                control::reset();
                minimap::reset();
                slot_mapping::reset();
                START_CHORD_WAS_DOWN.store(false, Ordering::Release);
                FINISH_CHORD_WAS_DOWN.store(false, Ordering::Release);
                FINISH_CONFIRM_ACTIVE.store(false, Ordering::Release);
                FINISH_CONFIRM_LMB_WAS_DOWN.store(false, Ordering::Release);
                STARTUP_SPEED_OVERRIDE_ACTIVE.store(false, Ordering::Release);
                TEMP_RELEASE_WAS_DOWN.store(false, Ordering::Release);
                SELECT_KEYS_WERE_DOWN.store(0, Ordering::Release);
                LMB_WAS_DOWN.store(false, Ordering::Release);
                RMB_WAS_DOWN.store(false, Ordering::Release);
            }
            true
        } else if session_was_active && control_scene {
            true
        } else {
            false
        };

        if session_was_active && !control_scene {
            MATCH_SESSION_ACTIVE.store(false, Ordering::Release);
            pacing_probe::prepare_next_match();
            control::reset();
            minimap::reset();
            slot_mapping::reset();
        }

        if session_active {
            pacing_probe::note_render_heartbeat();
        }

        let pause_ui = pause_probe::update(ctx, session_active);

        Self::poll_start_chord(control_scene);
        Self::poll_finish_chord(ingame);

        let mouse = self.read_mouse(ctx);
        Self::poll_finish_confirmation(mouse);
        let finish_confirm_active = FINISH_CONFIRM_ACTIVE.load(Ordering::Acquire);

        // Once an InGame session has started, any temporary non-InGame match scene is fail-closed.
        // The Ctrl+End confirmation also holds Candidate A so the user can make the irreversible
        // choice without the match advancing underneath the dialog.
        let presentation_paused =
            session_active && (!ingame || pause_ui.paused || finish_confirm_active);
        pacing_probe::set_presentation_state(session_active, presentation_paused);

        if !finish_confirm_active {
            Self::poll_player_selection(ctx, ingame);
            Self::poll_temporary_release(ingame);
            Self::poll_return_home(ctx, ingame);
            Self::poll_hold(ctx, ingame);
        }

        // Automatic fog follows the controlled champion's authoritative simulation team.
        // Stop enforcing on pause/release/spectator without changing the last native view.
        let vision_team = if ingame
            && pacing_probe::manual_input_enabled()
            && control::selected_athlete().is_some()
        {
            control::selected_team()
        } else {
            None
        };
        camera_probe::set_team_vision(vision_team);

        if !control_scene {
            return;
        }

        if !finish_confirm_active {
            let targeting_consumed_rmb = self.poll_targeting(ctx, mouse, ingame);
            self.poll_rmb_move(ctx, mouse, ingame, targeting_consumed_rmb);
        }

        if ingame {
            if !finish_confirm_active {
                Self::draw_targeting_overlays(ctx);
            }
            Self::draw_status(ctx, &pause_ui);

            if !pacing_probe::start_requested() && !pacing_probe::manual_control_released() {
                Self::draw_ready_prompt(ctx);
            }
            if finish_confirm_active {
                Self::draw_finish_confirmation(ctx);
            }
        } else {
            Self::draw_start_gate(ctx);
        }
    }
}

fn init(host: &StableHost) -> StableMod {
    match simulation_probe::ensure_installed() {
        Ok(()) => host.log(
            LogLevel::Info,
            "TFM2 Direct Control loaded (contextual RMB + minimap movement + A attack-move + manual skills + hold + return home)",
        ),
        Err(error) => host.log(
            LogLevel::Error,
            &format!("TFM2 Direct Control failed to initialize the supported simulation hook: {error}"),
        ),
    }

    let mut module = StableMod::new(MOD_ID);
    module.add_player_input_ai(pacing_probe::CandidateAObserverAi::default());
    module.set_extension(DirectControlExtension);
    module
}

declare_stable_mod!(init);
