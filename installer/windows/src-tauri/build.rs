use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    println!("cargo:rerun-if-env-changed=MINE_MAIL_NSIS_PAYLOAD");
    println!("cargo:rerun-if-env-changed=MINE_MAIL_RELEASE_VERSION");
    println!("cargo:rerun-if-env-changed=MINE_MAIL_ALLOW_EMPTY_PAYLOAD");

    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is required"));
    let main_tauri_config = manifest_dir.join("../../../web/src-tauri/tauri.conf.json");
    println!(
        "cargo:rerun-if-changed={}",
        main_tauri_config.to_string_lossy()
    );

    let release_version = env::var("MINE_MAIL_RELEASE_VERSION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| read_main_version(&main_tauri_config))
        .unwrap_or_else(|| "0.0.0-dev".to_owned());
    println!("cargo:rustc-env=MINE_MAIL_RELEASE_VERSION={release_version}");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is required"));
    let embedded_payload = out_dir.join("mine-mail-payload.exe");
    match env::var_os("MINE_MAIL_NSIS_PAYLOAD") {
        Some(payload) => {
            let payload = PathBuf::from(payload);
            if !payload.is_file() {
                panic!(
                    "MINE_MAIL_NSIS_PAYLOAD does not point to a file: {}",
                    payload.display()
                );
            }
            println!("cargo:rerun-if-changed={}", payload.to_string_lossy());
            fs::copy(&payload, &embedded_payload)
                .expect("failed to embed the Mine Mail NSIS payload");
        }
        None => {
            let release_build = env::var("PROFILE").as_deref() == Ok("release");
            let allow_empty = env::var("MINE_MAIL_ALLOW_EMPTY_PAYLOAD").as_deref() == Ok("1");
            if release_build && !allow_empty {
                panic!(
                    "release builds require MINE_MAIL_NSIS_PAYLOAD; \
                     set MINE_MAIL_ALLOW_EMPTY_PAYLOAD=1 only for visual previews"
                );
            }
            fs::write(&embedded_payload, [])
                .expect("failed to create the development payload placeholder");
        }
    }

    tauri_build::build();
}

fn read_main_version(config_path: &Path) -> Option<String> {
    let text = fs::read_to_string(config_path).ok()?;
    let config: serde_json::Value = serde_json::from_str(&text).ok()?;
    config
        .get("version")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}
