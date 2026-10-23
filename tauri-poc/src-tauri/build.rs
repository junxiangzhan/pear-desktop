fn main() {
    // Generates an `allow-<command>` permission per app command, so the remote
    // music.youtube.com origin can be granted exactly these and nothing else.
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "ipc_send",
            "ipc_invoke",
            "config_set",
            "config_set_partial",
            "config_edit",
            "plugins_manifest",
            "app_restart",
            "menu_update",
            "menu_set_checked",
            "prompt_open",
            "prompt_submit",
            "net_fetch",
            "window_set_always_on_top",
        ]),
    ))
    .expect("failed to run tauri-build");
}
