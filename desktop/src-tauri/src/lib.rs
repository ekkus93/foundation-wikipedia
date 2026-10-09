//! Thin Tauri adapter; future article services come from shared Rust.
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("Foundation Wikipedia desktop failed to start");
}
