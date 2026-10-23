fn main() {
    // Generates the `allow-save-config` permission so the remote
    // music.youtube.com origin can be granted this one command and nothing else.
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(&["save_config"])),
    )
    .expect("failed to run tauri-build");
}
