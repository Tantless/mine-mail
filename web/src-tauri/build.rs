use std::{env, fs, path::PathBuf};

const GOOGLE_OAUTH_FILE: &str = "google-oauth-client.json";
const GOOGLE_CLIENT_ID: &str =
    "609932488435-4h4fffcvl0hcpe0u9svc8k610tstvia7.apps.googleusercontent.com";

fn main() {
    println!("cargo:rerun-if-changed={GOOGLE_OAUTH_FILE}");
    let manifest_dir = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").expect("Cargo must provide its manifest directory"),
    );
    let credential_path = manifest_dir.join(GOOGLE_OAUTH_FILE);
    let client_secret = if credential_path.exists() {
        let contents = fs::read_to_string(&credential_path)
            .expect("Google OAuth client JSON could not be read");
        let document: serde_json::Value =
            serde_json::from_str(&contents).expect("Google OAuth client JSON is invalid");
        let installed = document
            .get("installed")
            .expect("Google OAuth client must have the Desktop app type");
        let client_id = installed
            .get("client_id")
            .and_then(serde_json::Value::as_str)
            .expect("Google OAuth client JSON is missing client_id");
        assert_eq!(
            client_id, GOOGLE_CLIENT_ID,
            "Google OAuth client JSON does not match Mine Mail's embedded client ID"
        );
        installed
            .get("client_secret")
            .and_then(serde_json::Value::as_str)
            .filter(|secret| !secret.trim().is_empty())
            .expect("Google OAuth client JSON is missing client_secret")
            .to_owned()
    } else {
        String::new()
    };
    let output_dir =
        PathBuf::from(env::var_os("OUT_DIR").expect("Cargo must provide its output directory"));
    fs::write(
        output_dir.join("google_oauth_config.rs"),
        format!("const GOOGLE_CLIENT_SECRET: &str = {client_secret:?};\n"),
    )
    .expect("generated Google OAuth configuration could not be written");

    tauri_build::build()
}
