package org.foundation.wikipedia.rust

import android.content.Context
import uniffi.wiki_ffi.hostedAiReady

/**
 * One-time initialisation of Android certificate verification for the shared
 * Rust HTTPS transport (AI-003 / MOB-001).
 *
 * rustls-platform-verifier checks server certificates with Android's own
 * trust manager (system CAs, revocation, the app's network security config).
 * It needs the application context once per process; until then Rust refuses
 * hosted connections with `HostedAiException.NotReady`.
 */
object RustTls {
    @Volatile
    private var initialized = false

    /** Idempotent and thread-safe. Throws if the verifier cannot initialise. */
    fun init(context: Context) {
        if (initialized) return
        synchronized(this) {
            if (initialized) return
            // JNA loads the library for UniFFI; JNI resolves this class's
            // native method only from libraries loaded through the runtime.
            System.loadLibrary("wiki_ffi")
            nativeInit(context.applicationContext)
            check(hostedAiReady()) { "Android certificate verifier did not initialise" }
            initialized = true
        }
    }

    /** Implemented in crates/wiki-ffi/src/android_tls.rs. */
    @JvmStatic
    private external fun nativeInit(context: Any)
}
