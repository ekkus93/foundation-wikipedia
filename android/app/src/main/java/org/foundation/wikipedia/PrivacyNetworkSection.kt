package org.foundation.wikipedia

import android.content.Context
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.selection.toggleable
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp

/**
 * Where this app's network traffic goes, in the order a user meets it. Each
 * line must stay true for the Android app as built (STATE-003); controls for
 * features Android does not have (online articles, audio/video) are not shown.
 */
internal val ANDROID_NETWORK_DISCLOSURES = listOf(
    "Articles: the reader shows articles only from installed packs and makes no network " +
        "requests while you read. Audio and video are not played.",
    "Official packs: the list and downloads come from GitHub, only after you agree to the " +
        "notice in Wikipedia & Offline Data. Over mobile data only if allowed below.",
    "AI chat: nothing is sent until you review a request and choose Send. A model server " +
        "on your local network receives the question and the listed passages (so the phone " +
        "is not offline); a hosted provider (Anthropic or OpenAI) receives them over the " +
        "internet. There is no fallback to another model.",
    "API keys are kept in Android's secure Keystore, never shown again and not backed up.",
    "The app sends no analytics or usage data.",
)

/** STATE-003 Android Privacy & Network page. */
@Composable
fun PrivacyNetworkSection() {
    val context = LocalContext.current
    val prefs = remember {
        context.getSharedPreferences(ReaderOfflinePreference.FILE, Context.MODE_PRIVATE)
    }
    var allowMobileData by remember {
        mutableStateOf(
            OfficialPackPreference.mobileDataAllowed(
                prefs.getString(OfficialPackPreference.MOBILE_DATA_KEY, null),
            ),
        )
    }
    var saveFailed by remember { mutableStateOf(false) }
    Text("Privacy & Network", style = MaterialTheme.typography.titleLarge)
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .toggleable(
                value = allowMobileData,
                role = Role.Switch,
                onValueChange = { allowed ->
                    val saved = prefs.edit()
                        .putString(
                            OfficialPackPreference.MOBILE_DATA_KEY,
                            OfficialPackPreference.encodeMobileData(allowed),
                        )
                        .commit()
                    if (saved) allowMobileData = allowed
                    saveFailed = !saved
                },
            )
            .padding(vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text("Download packs over mobile data", modifier = Modifier.weight(1f))
        Switch(checked = allowMobileData, onCheckedChange = null)
    }
    Text(
        if (allowMobileData) "Pack downloads may use mobile data and count toward your data plan."
        else "Pack downloads wait for Wi-Fi or another unmetered connection.",
        style = MaterialTheme.typography.bodySmall,
    )
    if (saveFailed) Text("This choice could not be saved; the previous setting stays.")
    Text("What goes over the network", style = MaterialTheme.typography.titleMedium)
    for (line in ANDROID_NETWORK_DISCLOSURES) Text("• $line")
}
