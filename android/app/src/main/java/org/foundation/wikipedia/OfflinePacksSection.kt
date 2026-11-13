package org.foundation.wikipedia

import android.content.Context
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import java.io.File
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.foundation.wikipedia.rust.RustOfflineLibrary
import uniffi.wiki_ffi.InstalledSnapshot
import uniffi.wiki_ffi.LibraryException
import uniffi.wiki_ffi.PackReview

/** One Rust library per process: it owns its data directory exclusively. */
object OfflineLibraryHolder {
    private val lock = Mutex()
    private var library: RustOfflineLibrary? = null

    suspend fun get(context: Context): RustOfflineLibrary = lock.withLock {
        library ?: RustOfflineLibrary.open(
            File(context.applicationContext.filesDir, "offline-library"),
        ).also { library = it }
    }
}

/** User-facing text for a failed pack operation; never claims publisher trust. */
fun packErrorMessage(error: Throwable): String = when (error) {
    is LibraryException.VerificationFailed ->
        "This pack failed verification: it is damaged or was modified. Nothing was installed."
    is LibraryException.UnsignedOfficialPack ->
        "This pack claims to be official, but publisher signatures cannot be verified yet, " +
            "so it was not installed."
    is LibraryException.ConfirmationRequired -> "Import cancelled. Nothing was installed."
    is LibraryException.AlreadyInstalled -> "This pack is already installed."
    is LibraryException.MediaNotSupported ->
        "This pack includes images or media, which cannot be installed yet."
    is LibraryException.SnapshotActive ->
        "This pack is in use. Activate another pack before removing it."
    is LibraryException.NotInstalled -> "That pack is no longer installed."
    is LibraryException.SourceFailed, is java.io.IOException ->
        "The selected file could not be read."
    is LibraryException.Storage -> "Storage error. Check free space and try again."
    is LibraryException.InvalidInput -> "That request was not valid."
    else -> "Something went wrong with the offline pack."
}

private fun formatBytes(bytes: ULong): String {
    val mib = bytes.toDouble() / (1024 * 1024)
    return if (mib >= 1) "%.1f MB".format(mib) else "${bytes} bytes"
}

/**
 * Pack manager for Settings › Wikipedia & Offline Data (MOBPACK-002):
 * import a custom `.wpack` with the system document picker, review its
 * verified status before installing, and activate or remove packs.
 */
@Composable
fun OfflinePacksSection() {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var library by remember { mutableStateOf<RustOfflineLibrary?>(null) }
    var unavailable by remember { mutableStateOf(false) }
    var packs by remember { mutableStateOf<List<InstalledSnapshot>>(emptyList()) }
    var busy by remember { mutableStateOf(false) }
    var message by remember { mutableStateOf<String?>(null) }
    var pendingReview by remember { mutableStateOf<PackReview?>(null) }
    var decision by remember { mutableStateOf<CompletableDeferred<Boolean>?>(null) }

    suspend fun refresh(lib: RustOfflineLibrary) {
        packs = lib.listPacks()
    }

    LaunchedEffect(Unit) {
        try {
            val lib = OfflineLibraryHolder.get(context)
            library = lib
            refresh(lib)
        } catch (error: LinkageError) {
            // Debug builds without libwiki_ffi.so packaged in jniLibs.
            unavailable = true
        } catch (error: Exception) {
            message = packErrorMessage(error)
        }
    }

    val picker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        val lib = library ?: return@rememberLauncherForActivityResult
        if (uri == null) return@rememberLauncherForActivityResult
        scope.launch {
            busy = true
            message = "Verifying pack…"
            message = try {
                val installed = context.contentResolver.openInputStream(uri)?.use { input ->
                    lib.importPack(input) { review ->
                        val answer = CompletableDeferred<Boolean>()
                        decision = answer
                        pendingReview = review
                        answer.await()
                    }
                } ?: throw java.io.IOException("unreadable document")
                refresh(lib)
                "Installed ${installed.snapshotId}. Activate it to read it offline."
            } catch (error: Exception) {
                packErrorMessage(error)
            } finally {
                pendingReview = null
                decision = null
                busy = false
            }
        }
    }

    Text("Offline packs", style = MaterialTheme.typography.titleMedium)
    if (unavailable) {
        Text("Offline packs are not available in this build (the shared Rust library is missing).")
        return
    }
    Button(
        onClick = { picker.launch(arrayOf("*/*")) },
        enabled = library != null && !busy,
        modifier = Modifier.fillMaxWidth(),
    ) { Text("Import a .wpack file") }
    Text(
        "Custom packs are checked for integrity before installing. They are never shown as " +
            "coming from a verified publisher.",
        style = MaterialTheme.typography.bodySmall,
    )
    message?.let { Text(it) }
    if (packs.isEmpty() && library != null) {
        Text("No offline packs are installed.")
    }
    for (pack in packs) {
        Column(modifier = Modifier.fillMaxWidth().padding(vertical = 8.dp)) {
            Text(
                pack.snapshotId + if (pack.active) " (active)" else "",
                style = MaterialTheme.typography.titleSmall,
            )
            Text(pack.trustLabel)
            if (!pack.damaged) {
                Text("${pack.project} · ${pack.articles} articles")
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                if (!pack.active && !pack.damaged) {
                    OutlinedButton(enabled = !busy, onClick = {
                        val lib = library ?: return@OutlinedButton
                        scope.launch {
                            busy = true
                            message = try {
                                lib.activate(pack.snapshotId)
                                refresh(lib)
                                "${pack.snapshotId} is now active."
                            } catch (error: Exception) {
                                packErrorMessage(error)
                            } finally {
                                busy = false
                            }
                        }
                    }) { Text("Activate") }
                }
                if (!pack.active) {
                    OutlinedButton(enabled = !busy, onClick = {
                        val lib = library ?: return@OutlinedButton
                        scope.launch {
                            busy = true
                            message = try {
                                lib.removePack(pack.snapshotId)
                                refresh(lib)
                                "Removed ${pack.snapshotId}."
                            } catch (error: Exception) {
                                packErrorMessage(error)
                            } finally {
                                busy = false
                            }
                        }
                    }) { Text("Remove") }
                }
            }
        }
    }

    pendingReview?.let { review ->
        AlertDialog(
            onDismissRequest = { decision?.complete(false) },
            title = { Text("Install custom pack?") },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Text(review.trustLabel, style = MaterialTheme.typography.titleSmall)
                    Text("Pack: ${review.snapshotId}")
                    Text("Wikipedia edition: ${review.project}")
                    Text("Articles: ${review.articles}")
                    Text("Size: ${formatBytes(review.declaredBytes)}")
                    Text(
                        "Its contents passed integrity checks, but it is not from a verified " +
                            "publisher. Install it only if you trust where it came from.",
                    )
                }
            },
            confirmButton = {
                TextButton(onClick = { decision?.complete(true) }) { Text("Install") }
            },
            dismissButton = {
                TextButton(onClick = { decision?.complete(false) }) { Text("Cancel") }
            },
        )
    }
}
