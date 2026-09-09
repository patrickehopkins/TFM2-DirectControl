use std::{
    collections::VecDeque,
    fmt::Write as _,
    fs,
    path::PathBuf,
    sync::Mutex,
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
        Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON},
        WindowsAndMessaging::{
            GetClientRect, GetCursorPos, GetForegroundWindow, GetWindowThreadProcessId,
        },
    },
};

const MOD_ID: &str = "tfm2_direct_control";
const UI_FALLBACK_W: f32 = 1920.0;
const UI_FALLBACK_H: f32 = 1080.0;
const UI_DUMP_FILENAME: &str = "TFM2-DirectControl-ui-tree.txt";
const UI_DUMP_MAX_DEPTH: usize = 24;
const UI_DUMP_MAX_NODES: usize = 12_000;
const STATUS_FRAMES: u32 = 360;

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
struct DiagnosticStatus {
    text: String,
    frames_left: u32,
}

static DIAGNOSTIC_STATUS: Mutex<Option<DiagnosticStatus>> = Mutex::new(None);

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

    fn dump_path() -> PathBuf {
        std::env::temp_dir().join(UI_DUMP_FILENAME)
    }

    fn clean_inline(value: String, max_chars: usize) -> String {
        let mut result = value.replace(['\r', '\n', '\t'], " ");
        if result.chars().count() > max_chars {
            result = result.chars().take(max_chars).collect();
            result.push_str("...");
        }
        result
    }

    fn format_rect(rect: Option<(f32, f32, f32, f32)>) -> String {
        match rect {
            Some((x, y, w, h)) => format!("({x:.1},{y:.1},{w:.1},{h:.1})"),
            None => "-".to_owned(),
        }
    }

    fn looks_camera_relevant(
        path: &str,
        runner: &str,
        rect: Option<(f32, f32, f32, f32)>,
        contents_rect: Option<(f32, f32, f32, f32)>,
    ) -> bool {
        let name = format!("{path} {runner}").to_ascii_lowercase();
        const KEYWORDS: &[&str] = &[
            "camera",
            "viewport",
            "view_port",
            "view",
            "minimap",
            "mini_map",
            "map",
            "field",
            "battle",
            "match",
            "game",
            "world",
            "scene",
            "spectat",
            "render",
            "canvas",
        ];

        if KEYWORDS.iter().any(|keyword| name.contains(keyword)) {
            return true;
        }

        let geometry_candidate = |(_, _, w, h): (f32, f32, f32, f32)| {
            let large_panel = w >= 700.0 && h >= 400.0;
            let square_panel =
                (140.0..=500.0).contains(&w)
                    && (140.0..=500.0).contains(&h)
                    && (w - h).abs() <= 100.0;
            large_panel || square_panel
        };

        rect.map(geometry_candidate).unwrap_or(false)
            || contents_rect.map(geometry_candidate).unwrap_or(false)
    }

    fn dump_ui_tree(ctx: &StableClient<'_>) -> Result<(PathBuf, usize, usize), String> {
        let mut queue = VecDeque::new();
        queue.push_back((String::new(), 0usize));

        let mut full = String::new();
        let mut candidates = String::new();
        let mut visited = 0usize;
        let mut candidate_count = 0usize;

        writeln!(full, "TFM2 Direct Control - live UI tree dump").ok();
        writeln!(full, "Scene: {:?}", ctx.client_scene_kind()).ok();
        writeln!(full, "Root children: {:?}", ctx.ui_child_names("")).ok();
        writeln!(full).ok();

        while let Some((path, depth)) = queue.pop_front() {
            if visited >= UI_DUMP_MAX_NODES {
                writeln!(full, "\n[TRUNCATED after {UI_DUMP_MAX_NODES} nodes]").ok();
                break;
            }
            if depth > UI_DUMP_MAX_DEPTH {
                continue;
            }

            let children = ctx.ui_child_names(&path);
            let runner = ctx.ui_runner_name(&path).unwrap_or_default();
            let visible = ctx.ui_visible(&path);
            let rect = ctx.ui_node_rect(&path);
            let contents_rect = ctx.ui_contents_rect(&path);
            let text = ctx
                .ui_text(&path)
                .map(|value| Self::clean_inline(value, 240))
                .unwrap_or_default();
            let state = ctx
                .ui_state_json(&path)
                .map(|value| Self::clean_inline(value, 1_000))
                .unwrap_or_default();

            let display_path = if path.is_empty() { "<root>" } else { &path };
            let line = format!(
                "depth={depth:02} path={display_path} | runner={runner:?} | visible={visible:?} | rect={} | contents={} | children={}{}{}",
                Self::format_rect(rect),
                Self::format_rect(contents_rect),
                children.len(),
                if text.is_empty() {
                    String::new()
                } else {
                    format!(" | text={text:?}")
                },
                if state.is_empty() {
                    String::new()
                } else {
                    format!(" | state={state}")
                },
            );

            writeln!(full, "{line}").ok();
            if Self::looks_camera_relevant(&path, &runner, rect, contents_rect) {
                candidate_count += 1;
                writeln!(candidates, "{line}").ok();
            }

            visited += 1;
            if depth < UI_DUMP_MAX_DEPTH {
                for child in children {
                    let child_path = if path.is_empty() {
                        child
                    } else {
                        format!("{path}.{child}")
                    };
                    queue.push_back((child_path, depth + 1));
                }
            }
        }

        let mut report = String::new();
        writeln!(report, "TFM2 DIRECT CONTROL - CAMERA/UI DISCOVERY REPORT").ok();
        writeln!(report, "Visited nodes: {visited}").ok();
        writeln!(report, "Candidate nodes: {candidate_count}").ok();
        writeln!(report).ok();
        writeln!(report, "========== CAMERA / MAP / VIEW CANDIDATES ==========").ok();
        if candidates.is_empty() {
            writeln!(report, "<none>").ok();
        } else {
            report.push_str(&candidates);
        }
        writeln!(report, "\n========== COMPLETE UI TREE ==========").ok();
        report.push_str(&full);

        let path = Self::dump_path();
        fs::write(&path, report)
            .map_err(|error| format!("failed to write {}: {error}", path.display()))?;

        Ok((path, visited, candidate_count))
    }

    fn set_status(text: String) {
        if let Ok(mut status) = DIAGNOSTIC_STATUS.lock() {
            *status = Some(DiagnosticStatus {
                text,
                frames_left: STATUS_FRAMES,
            });
        }
    }

    fn draw_status(ctx: &mut StableClient<'_>) {
        let message = {
            let Ok(mut status) = DIAGNOSTIC_STATUS.lock() else {
                return;
            };
            let Some(current) = status.as_mut() else {
                return;
            };
            if current.frames_left == 0 {
                *status = None;
                return;
            }
            current.frames_left -= 1;
            current.text.clone()
        };

        ctx.draw_rect("UI", 18.0, 94.0, 1_050.0, 30.0, 19_998, 6.0, 0x101018dd);
        ctx.draw_text(
            "UI",
            &message,
            "asset/base/font/set/regular",
            (28.0, 94.0, 1_030.0, 30.0),
            19_999,
            13.0,
            0xffffffff,
            TextAlignXV1::Left,
            TextAlignYV1::Center,
        );
    }
}

impl StableExtension for DirectControlExtension {
    fn post_render(&self, ctx: &mut StableClient<'_>) {
        if !Self::should_draw(ctx) {
            return;
        }

        if matches!(ctx.client_scene_kind(), Some(ClientSceneKindV1::InGame))
            && ctx.key_pressed("F12")
        {
            match Self::dump_ui_tree(ctx) {
                Ok((path, nodes, candidates)) => Self::set_status(format!(
                    "UI dump written: {} | {} nodes | {} candidates",
                    path.display(),
                    nodes,
                    candidates
                )),
                Err(error) => Self::set_status(format!("UI dump FAILED: {error}")),
            }
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
        }

        if matches!(ctx.client_scene_kind(), Some(ClientSceneKindV1::InGame)) {
            ctx.draw_rect("UI", 18.0, 56.0, 760.0, 30.0, 19_998, 6.0, 0x101018cc);
            ctx.draw_text(
                "UI",
                "Camera discovery: press F12 once to dump the live UI tree to %TEMP%\\TFM2-DirectControl-ui-tree.txt",
                "asset/base/font/set/regular",
                (28.0, 56.0, 740.0, 30.0),
                19_999,
                13.0,
                0xffffffff,
                TextAlignXV1::Left,
                TextAlignYV1::Center,
            );
        }

        Self::draw_status(ctx);
    }
}

fn init(host: &StableHost) -> StableMod {
    host.log(
        LogLevel::Info,
        "TFM2 Direct Control loaded (mouse + UI-tree camera discovery build)",
    );

    let mut module = StableMod::new(MOD_ID);
    module.set_extension(DirectControlExtension);
    module
}

declare_stable_mod!(init);
