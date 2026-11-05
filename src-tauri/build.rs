fn main() {
    let attributes = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(&["native_context_menu"]));
    tauri_build::try_build(attributes).expect("cannot prepare native desktop bundle");
}
