//! Platform-neutral dependency boundary for eventual UniFFI Android bindings.
//!
//! The crate produces an Android-loadable cdylib but does not export JNI or
//! callable C symbols yet. The workspace's strict unsafe-code lint is retained;
//! any future ABI export requires a separately reviewed FFI safety boundary.
//! Native ELF compilation is not equivalent to a working Kotlin/UniFFI API.
