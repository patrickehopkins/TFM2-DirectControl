use std::{
    fs,
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};

use mod_api::*;

const MOD_ID: &str = "tfm2_camera_probe";
const DUMP_FILE: &str = "TFM2-DirectControl-classic-camera-probe.txt";
const DUMP_INTERVAL: Duration = Duration::from_secs(1);

static LAST_DUMP: Mutex<Option<Instant>> = Mutex::new(None);

#[derive(Default)]
struct CameraProbeExtension;

impl CameraProbeExtension {
    fn dump_path() -> PathBuf {
        std::env::temp_dir().join(DUMP_FILE)
    }

    fn due() -> bool {
        let Ok(mut guard) = LAST_DUMP.lock() else {
            return false;
        };

        let now = Instant::now();
        match *guard {
            Some(last) if now.duration_since(last) < DUMP_INTERVAL => false,
            _ => {
                *guard = Some(now);
                true
            }
        }
    }
}

impl ModExtension for CameraProbeExtension {
    fn post_render(
        &self,
        scene: &Scene,
        _ui: &GameUI,
        _assets: &Assets,
        state: &mut RenderState,
    ) {
        if !Self::due() {
            return;
        }

        // This probe intentionally asks the classic SDK whether its internal client
        // types implement Debug. If they do, the output gives us the live camera/render
        // object graph without guessing private field names. If either type does not
        // implement Debug, the compiler error itself tells us exactly which surface is
        // opaque and we move to the next inspection technique.
        let report = format!(
            "TFM2 Direct Control - classic 0.5 camera probe\n\n\
             Scene type: {}\n\
             Scene size: {} bytes\n\
             RenderState type: {}\n\
             RenderState size: {} bytes\n\n\
             ===== SCENE DEBUG =====\n{scene:#?}\n\n\
             ===== RENDER STATE DEBUG =====\n{state:#?}\n",
            std::any::type_name_of_val(scene),
            std::mem::size_of_val(scene),
            std::any::type_name_of_val(state),
            std::mem::size_of_val(state),
        );

        if let Err(error) = fs::write(Self::dump_path(), report) {
            eprintln!("TFM2 Camera Probe: failed to write dump: {error}");
        }
    }
}

fn init(_ctx: &GameCtx) -> ModRegistration {
    let mut registration = ModRegistration::new(MOD_ID);
    registration.set_extension(CameraProbeExtension);
    registration
}

declare_mod!(init);
