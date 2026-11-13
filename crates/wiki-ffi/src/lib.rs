//! Shared-core boundary for the Android app, exported through UniFFI.
//!
//! `android` holds the Kotlin-callable API (MOB-001). Per the owner decision
//! of 2026-11-13 this crate relaxes the workspace `unsafe_code = "forbid"` to
//! "deny" only so UniFFI's generated scaffolding (which carries its own scoped
//! allowance) compiles; hand-written `unsafe` remains rejected and is guarded
//! by scripts/test_ffi_unsafe_boundary.py.

pub mod android;
#[cfg(target_os = "android")]
pub mod android_tls;
pub mod article_chat;
pub mod online_reader;

uniffi::setup_scaffolding!();
