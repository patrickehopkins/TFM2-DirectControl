use mod_api_stable::{
    declare_stable_mod, ClientSceneKindV1, LogLevel, StableClient, StableExtension, StableHost,
    StableMatchHook, StableMod, StableSim, TextAlignXV1, TextAlignYV1,
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

const WORLD_CENTER: u64 = 480_000;
const WORLD_PROBE_OFFSET: u64 = 120_000;
const WORLD_MARKER_RADIUS: u64 = 6_000;
const WORLD_MARKER_HALF_LINE: u64 = 12_000;

const COLOR_YELLOW: u32 = 0xffe040ff;
const COLOR_CYAN: u32 = 0x40e0ffff;
const COLOR_MAGENTA: u32 = 0xff40e0ff;

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
        // TFM2's stable API currently exposes raw keyboard input but not raw mouse
        // coordinates/buttons. Keep all Windows-specific input acquisition isolated here.
        //
        // Only use the foreground window when it belongs to this game process. That
        // prevents clicks on another monitor/application from being interpreted as TFM2
        // coordinates and also ensures future manual controls are inactive while alt-tabbed.
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

            // TFM2 can remain foreground while the physical cursor crosses onto another
            // monitor. Treat anything outside the game's own client area as inactive so a
            // future RMB cannot become an off-window gameplay command.
            if cursor.x < 0 || cursor.y < 0 || cursor.x >= client_w || cursor.y >= client_h {
                return MouseSnapshot::default();
            }

            let (ui_w, ui_h) = ctx
                .draw_map_size("UI")
                .unwrap_or((UI_FALLBACK_W, UI_FALLBACK_H));

            let ui_x = cursor.x as f32 * ui_w / client_w as f32;
            let ui_y = cursor.y as f32 * ui_h / client_h as f32;

            MouseSnapshot {
                valid: true,
                ui_x,
                ui_y,
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
}

impl StableExtension for DirectControlExtension {
    fn post_render(&self, ctx: &mut StableClient<'_>) {
        if !Self::should_draw(ctx) {
            return;
        }

        let mouse = self.read_mouse(ctx);
        if !mouse.valid {
            return;
        }

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

        ctx.draw_rect("UI", 18.0, 18.0, 760.0, 34.0, 19_998, 6.0, 0x101018dd);
        ctx.draw_text(
            "UI",
            &label,
            "asset/base/font/set/regular",
            (28.0, 18.0, 740.0, 34.0),
            19_999,
            14.0,
            0xffffffff,
            TextAlignXV1::Left,
            TextAlignYV1::Center,
        );

        if matches!(ctx.client_scene_kind(), Some(ClientSceneKindV1::InGame)) {
            ctx.draw_rect("UI", 18.0, 56.0, 700.0, 30.0, 19_998, 6.0, 0x101018cc);
            ctx.draw_text(
                "UI",
                "SIM world probe: YELLOW center (480k,480k) | CYAN +X | MAGENTA +Y",
                "asset/base/font/set/regular",
                (28.0, 56.0, 680.0, 30.0),
                19_999,
                13.0,
                0xffffffff,
                TextAlignXV1::Left,
                TextAlignYV1::Center,
            );
        }
    }
}

#[derive(Debug, Default)]
struct WorldProbeHook;

impl WorldProbeHook {
    fn draw_cross(sim: &mut StableSim<'_>, x: u64, y: u64, color: u32) {
        sim.debug_draw_circle(x, y, WORLD_MARKER_RADIUS, color);
        sim.debug_draw_line(
            x.saturating_sub(WORLD_MARKER_HALF_LINE),
            y,
            x.saturating_add(WORLD_MARKER_HALF_LINE),
            y,
            color,
        );
        sim.debug_draw_line(
            x,
            y.saturating_sub(WORLD_MARKER_HALF_LINE),
            x,
            y.saturating_add(WORLD_MARKER_HALF_LINE),
            color,
        );
    }
}

impl StableMatchHook for WorldProbeHook {
    fn on_match_start(&self, _sim: &mut StableSim<'_>) {}

    fn on_match_tick(&self, sim: &mut StableSim<'_>, _rng_seed: u64) {
        // StableSim debug drawing is explicitly defined in simulation world coordinates.
        // These anchors should therefore stay glued to fixed terrain positions while the
        // spectator camera pans or zooms.
        Self::draw_cross(sim, WORLD_CENTER, WORLD_CENTER, COLOR_YELLOW);
        Self::draw_cross(
            sim,
            WORLD_CENTER + WORLD_PROBE_OFFSET,
            WORLD_CENTER,
            COLOR_CYAN,
        );
        Self::draw_cross(
            sim,
            WORLD_CENTER,
            WORLD_CENTER + WORLD_PROBE_OFFSET,
            COLOR_MAGENTA,
        );
    }

    fn check_match_end(&self, _sim: &mut StableSim<'_>) -> Option<bool> {
        None
    }
}

fn init(host: &StableHost) -> StableMod {
    host.log(
        LogLevel::Info,
        "TFM2 Direct Control loaded (mouse + simulation world diagnostic build)",
    );

    let mut module = StableMod::new(MOD_ID);
    module.set_extension(DirectControlExtension);
    module.set_match_hook(WorldProbeHook);
    module
}

declare_stable_mod!(init);
