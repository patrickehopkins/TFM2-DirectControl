use mod_api_stable::{declare_stable_mod, LogLevel, StableHost, StableMod};

const MOD_ID: &str = "tfm2_direct_control";

fn init(host: &StableHost) -> StableMod {
    host.log(
        LogLevel::Info,
        "TFM2 Direct Control loaded (bootstrap build; direct controls not active yet)",
    );

    StableMod::new(MOD_ID)
}

declare_stable_mod!(init);
