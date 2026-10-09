//! Platform-neutral bootstrap boundary for future UniFFI-generated bindings.
//!
//! The only currently exported native ABI symbol is a version handshake. No
//! article/search API or JNI/UniFFI Kotlin bindings are exposed yet.
/// Version of the provisional native symbol contract (NOT pack format).
pub const NATIVE_ABI_VERSION: u32 = 1;

/// Safe C-ABI handshake to verify that Android's native artifact actually
/// exports a callable symbol. Not a JNI entry point or a working UniFFI API.
#[no_mangle]
pub extern "C" fn foundation_wikipedia_ffi_abi_version() -> u32 {
    NATIVE_ABI_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_abi_symbol_matches_declared_version() {
        assert_eq!(foundation_wikipedia_ffi_abi_version(), 1);
    }
}
