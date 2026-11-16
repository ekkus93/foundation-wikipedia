package org.foundation.wikipedia

import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.selection.toggleable
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import org.foundation.wikipedia.rust.RustOfflineLibrary
import uniffi.wiki_ffi.OfficialCatalogInfo
import uniffi.wiki_ffi.OfficialPackInfo
import uniffi.wiki_ffi.PackReview
import uniffi.wiki_ffi.PackTrust

/** The active network for download policy (needs ACCESS_NETWORK_STATE). */
fun currentDownloadNetwork(context: Context): DownloadNetwork {
    val manager = context.getSystemService(ConnectivityManager::class.java)
        ?: return DownloadNetwork.NONE
    val capabilities = manager.getNetworkCapabilities(manager.activeNetwork)
        ?: return DownloadNetwork.NONE
    if (!capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)) {
        return DownloadNetwork.NONE
    }
    return if (manager.isActiveNetworkMetered) DownloadNetwork.METERED else DownloadNetwork.UNMETERED
}

/** Bytes the app could allocate for downloads, counting clearable cache. */
fun allocatableBytes(context: Context): Long = try {
    val storage = context.getSystemService(android.os.storage.StorageManager::class.java)
    storage.getAllocatableBytes(storage.getUuidForPath(context.filesDir))
} catch (_: Exception) {
    context.filesDir.usableSpace
}

private data class DownloadProgress(val packId: String, val done: Long, val total: Long)

/**
 * MOBPACK-001 official packs in Settings › Wikipedia & Offline Data: after a
 * one-time privacy disclosure, check the signed catalog, then download a pack
 * (resumable, Wi-Fi only unless mobile data is allowed, with a free-space
 * check) and review it as "Verified publisher" before it is installed.
 */
@Composable
fun OfficialPacksSection(
    library: RustOfflineLibrary,
    enabled: Boolean,
    onInstalled: suspend () -> Unit,
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val prefs = remember {
        context.getSharedPreferences(ReaderOfflinePreference.FILE, Context.MODE_PRIVATE)
    }
    var disclosureAccepted by remember {
        mutableStateOf(
            OfficialPackPreference.disclosureAccepted(
                prefs.getString(OfficialPackPreference.DISCLOSURE_KEY, null),
            ),
        )
    }
    var allowMobileData by remember {
        mutableStateOf(
            OfficialPackPreference.mobileDataAllowed(
                prefs.getString(OfficialPackPreference.MOBILE_DATA_KEY, null),
            ),
        )
    }
    var catalog by remember { mutableStateOf<OfficialCatalogInfo?>(null) }
    var busy by remember { mutableStateOf(false) }
    var message by remember { mutableStateOf<String?>(null) }
    var progress by remember { mutableStateOf<DownloadProgress?>(null) }
    var job by remember { mutableStateOf<Job?>(null) }
    var askDisclosure by remember { mutableStateOf<(() -> Unit)?>(null) }
    var pendingReview by remember { mutableStateOf<PackReview?>(null) }
    var decision by remember { mutableStateOf<CompletableDeferred<Boolean>?>(null) }

    LaunchedEffect(library) {
        catalog = try {
            library.officialCatalog()
        } catch (error: Exception) {
            message = packErrorMessage(error)
            null
        }
    }

    /** Run [action] only if policy allows, asking for the disclosure first. */
    fun gated(pack: OfficialPackInfo?, action: () -> Unit) {
        val gate = officialDownloadGate(
            disclosureAccepted = disclosureAccepted,
            workingOffline = ReaderOfflinePreference.decode(
                prefs.getString(ReaderOfflinePreference.KEY, null),
            ),
            network = currentDownloadNetwork(context),
            allowMobileData = allowMobileData,
            neededBytes = pack?.requiredFreeBytes?.toLong(),
            availableBytes = pack?.let { allocatableBytes(context) },
        )
        when (gate) {
            DownloadGate.Allowed -> action()
            DownloadGate.NeedsDisclosure -> askDisclosure = { gated(pack, action) }
            else -> message = downloadGateMessage(gate)
        }
    }

    fun refreshCatalog() = gated(null) {
        job = scope.launch {
            busy = true
            message = "Checking for official packs…"
            message = try {
                catalog = library.refreshOfficialCatalog(context)
                val count = catalog?.packs?.size ?: 0
                if (count == 0) "No official packs are available yet." else null
            } catch (error: Exception) {
                packErrorMessage(error)
            } finally {
                busy = false
                job = null
            }
        }
    }

    fun download(pack: OfficialPackInfo) = gated(pack) {
        job = scope.launch {
            busy = true
            message = null
            progress = DownloadProgress(pack.packId, pack.downloadedBytes.toLong(), pack.bytes.toLong())
            message = try {
                val installed = library.downloadOfficialPack(
                    context,
                    pack.packId,
                    onProgress = { done, total ->
                        progress = DownloadProgress(pack.packId, done, total)
                    },
                ) { review ->
                    val answer = CompletableDeferred<Boolean>()
                    decision = answer
                    pendingReview = review
                    answer.await()
                }
                onInstalled()
                "Installed ${installed.snapshotId}. Activate it to read it offline."
            } catch (error: kotlinx.coroutines.CancellationException) {
                "Download paused. Choose Resume to continue."
            } catch (error: Exception) {
                packErrorMessage(error)
            } finally {
                pendingReview = null
                decision = null
                progress = null
                busy = false
                job = null
                catalog = runCatching { library.officialCatalog() }.getOrNull() ?: catalog
            }
        }
    }

    Text(
        "Official packs",
        style = MaterialTheme.typography.titleMedium,
        modifier = Modifier.padding(top = 16.dp),
    )
    Text(
        "Packs published by the project, checked against the publisher's signature " +
            "before they are installed.",
        style = MaterialTheme.typography.bodySmall,
    )
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .toggleable(
                value = allowMobileData,
                role = Role.Switch,
                onValueChange = { allowed ->
                    if (prefs.edit()
                            .putString(
                                OfficialPackPreference.MOBILE_DATA_KEY,
                                OfficialPackPreference.encodeMobileData(allowed),
                            )
                            .commit()
                    ) {
                        allowMobileData = allowed
                    }
                },
            )
            .padding(vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text("Download over mobile data", modifier = Modifier.weight(1f))
        Switch(checked = allowMobileData, onCheckedChange = null)
    }
    Button(
        onClick = { refreshCatalog() },
        enabled = enabled && !busy,
        modifier = Modifier.fillMaxWidth(),
    ) { Text("Check for official packs") }

    progress?.let { current ->
        val fraction = if (current.total > 0) current.done.toFloat() / current.total else 0f
        val text = "Downloading ${current.packId}: ${formatStorage(current.done)} of " +
            formatStorage(current.total)
        Column(Modifier.fillMaxWidth().padding(vertical = 8.dp)) {
            LinearProgressIndicator(
                progress = { fraction },
                modifier = Modifier.fillMaxWidth().semantics { contentDescription = text },
            )
            Text(text, style = MaterialTheme.typography.bodySmall)
            OutlinedButton(onClick = { job?.cancel() }) { Text("Cancel download") }
        }
    }
    message?.let { Text(it) }

    val current = catalog
    if (current == null) {
        Text("No official pack list yet.", style = MaterialTheme.typography.bodySmall)
    } else {
        Text(
            "Pack list from ${current.publisher}, published ${current.generated}.",
            style = MaterialTheme.typography.bodySmall,
        )
        for (pack in current.packs) {
            Column(
                modifier = Modifier.fillMaxWidth().padding(vertical = 8.dp),
                verticalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                Text(pack.title, style = MaterialTheme.typography.titleSmall)
                Text(
                    "${pack.project} · snapshot ${pack.snapshot} · " +
                        formatStorage(pack.bytes.toLong()),
                    style = MaterialTheme.typography.bodySmall,
                )
                if (pack.installed) {
                    Text("Installed — see the installed packs list for its status.")
                } else {
                    PackTrustBadge(PackTrust.NOT_YET_CHECKED, showDescription = false)
                    Text(
                        "Its signature and every file are checked after download, " +
                            "before anything is installed.",
                        style = MaterialTheme.typography.bodySmall,
                    )
                    if (pack.downloadedBytes > 0u) {
                        Text(
                            "Partly downloaded: ${formatStorage(pack.downloadedBytes.toLong())} " +
                                "of ${formatStorage(pack.bytes.toLong())}.",
                            style = MaterialTheme.typography.bodySmall,
                        )
                    }
                    Text(
                        "Needs ${formatStorage(pack.requiredFreeBytes.toLong())} of free space.",
                        style = MaterialTheme.typography.bodySmall,
                    )
                    OutlinedButton(
                        enabled = enabled && !busy,
                        onClick = { download(pack) },
                    ) {
                        Text(if (pack.downloadedBytes > 0u) "Resume download" else "Download")
                    }
                }
            }
        }
    }

    askDisclosure?.let { proceed ->
        AlertDialog(
            onDismissRequest = { askDisclosure = null },
            title = { Text("Download from GitHub?") },
            text = { Text(OFFICIAL_PACK_DISCLOSURE) },
            confirmButton = {
                TextButton(onClick = {
                    askDisclosure = null
                    val saved = prefs.edit()
                        .putString(
                            OfficialPackPreference.DISCLOSURE_KEY,
                            OfficialPackPreference.encodeDisclosure(),
                        )
                        .commit()
                    if (saved) {
                        disclosureAccepted = true
                        proceed()
                    } else {
                        message = "Storage error. Check free space and try again."
                    }
                }) { Text("Continue") }
            },
            dismissButton = {
                TextButton(onClick = { askDisclosure = null }) { Text("Not now") }
            },
        )
    }
    pendingReview?.let { review ->
        PackReviewDialog(review) { approved -> decision?.complete(approved) }
    }
}
