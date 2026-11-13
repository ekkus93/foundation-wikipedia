//! Workspace-pinned UniFFI bindings generator (MOB-001).
//!
//! Usage (library mode, reading the exported API from the built cdylib):
//! `cargo run -p uniffi-bindgen -- generate --library target/debug/libwiki_ffi.so --language kotlin --out-dir OUT`

fn main() {
    uniffi::uniffi_bindgen_main()
}
