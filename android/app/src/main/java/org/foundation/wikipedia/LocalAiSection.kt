package org.foundation.wikipedia

import android.content.Context
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import org.foundation.wikipedia.rust.RustLocalAi
import uniffi.wiki_ffi.ChatLocality
import uniffi.wiki_ffi.LocalAiException
import uniffi.wiki_ffi.LocalAiProtocol

/** A chosen local model server and model; not secret. */
data class LocalAiChoice(val protocol: LocalAiProtocol, val endpoint: String, val model: String)

/** The saved local model choice (MOB-003). Malformed values mean "none". */
object LocalAiPreference {
    private const val FILE = "foundation_local_ai"

    fun load(context: Context): LocalAiChoice? = try {
        val prefs = context.getSharedPreferences(FILE, Context.MODE_PRIVATE)
        val protocol = LocalAiProtocol.entries.firstOrNull { it.name == prefs.getString("protocol", null) }
        val endpoint = prefs.getString("endpoint", null)
        val model = prefs.getString("model", null)
        if (protocol == null || endpoint.isNullOrBlank() || model.isNullOrBlank()) null
        else LocalAiChoice(protocol, endpoint, model)
    } catch (_: ClassCastException) {
        null
    }

    fun save(context: Context, choice: LocalAiChoice?): Boolean {
        val editor = context.getSharedPreferences(FILE, Context.MODE_PRIVATE).edit()
        if (choice == null) {
            editor.clear()
        } else {
            editor.putString("protocol", choice.protocol.name)
                .putString("endpoint", choice.endpoint)
                .putString("model", choice.model)
        }
        return editor.commit()
    }
}

/** User-facing text for a failed local server check. */
fun localAiErrorMessage(error: Throwable): String = when (error) {
    is LocalAiException.InvalidEndpoint ->
        "Enter an http:// address with an IP on this device or your local network, " +
            "such as http://192.168.1.20:11434."
    is LocalAiException.Unreachable -> "Nothing answered at that address. Is the server running?"
    is LocalAiException.Timeout -> "The server did not answer in time."
    is LocalAiException.InvalidResponse -> "That server did not answer like the chosen protocol."
    is LocalAiException.NoModels -> "The server has no models. Download one on the server first."
    is LocalAiException.Cancelled -> "Stopped."
    is LinkageError -> "Local models are not available in this build."
    else -> "Could not reach the local model server."
}

fun ChatLocality.describe(): String = when (this) {
    ChatLocality.CLOUD -> "a hosted service on the internet"
    ChatLocality.LOCAL_NETWORK -> "a computer on your local network"
    ChatLocality.THIS_DEVICE -> "a server on this device"
}

private fun LocalAiProtocol.label() = when (this) {
    LocalAiProtocol.OLLAMA -> "Ollama"
    LocalAiProtocol.OPEN_AI_COMPATIBLE -> "OpenAI-compatible (llama.cpp, LM Studio…)"
}

/**
 * Settings › AI Assistant › Local or LAN model: find the models of a server
 * on this device or the local network and choose one for article chat.
 */
@Composable
fun LocalAiSection() {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val local = remember { RustLocalAi() }
    val saved = remember { LocalAiPreference.load(context) }
    var choice by remember { mutableStateOf(saved) }
    var protocol by remember { mutableStateOf(saved?.protocol ?: LocalAiProtocol.OLLAMA) }
    var endpoint by remember { mutableStateOf(saved?.endpoint ?: "") }
    var models by remember { mutableStateOf<List<String>?>(null) }
    var locality by remember { mutableStateOf<ChatLocality?>(null) }
    var busy by remember { mutableStateOf(false) }
    var message by remember { mutableStateOf<String?>(null) }

    Text("Local or LAN model", style = MaterialTheme.typography.titleMedium)
    Text(
        "Optional. Use a model server you run, such as Ollama on a computer on the same " +
            "network. Questions and article excerpts go to that computer, not the internet. " +
            "On the Android emulator, the computer running it is http://10.0.2.2:11434.",
        style = MaterialTheme.typography.bodySmall,
    )
    for (option in LocalAiProtocol.entries) {
        Row(
            modifier = Modifier.fillMaxWidth().clickable(enabled = !busy) {
                protocol = option
                models = null
            },
            verticalAlignment = Alignment.CenterVertically,
        ) {
            RadioButton(selected = protocol == option, onClick = null)
            Text(option.label())
        }
    }
    OutlinedTextField(
        value = endpoint,
        onValueChange = {
            endpoint = it.take(200).trim()
            models = null
        },
        label = { Text("Server address") },
        placeholder = { Text("http://192.168.1.20:11434") },
        singleLine = true,
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, autoCorrect = false),
        modifier = Modifier.fillMaxWidth(),
    )
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        OutlinedButton(enabled = !busy && endpoint.isNotBlank(), onClick = {
            scope.launch {
                busy = true
                message = null
                try {
                    val server = local.discover(protocol, endpoint)
                    models = server.models()
                    locality = server.locality()
                    server.close()
                } catch (error: Exception) {
                    models = null
                    message = localAiErrorMessage(error)
                } catch (error: LinkageError) {
                    message = localAiErrorMessage(error)
                } finally {
                    busy = false
                }
            }
        }) { Text("Find models") }
        if (choice != null) {
            OutlinedButton(enabled = !busy, onClick = {
                if (LocalAiPreference.save(context, null)) {
                    choice = null
                    message = "Local model removed."
                }
            }) { Text("Remove") }
        }
    }
    Text(
        "Find models asks the server for its model list; no question or article text is sent.",
        style = MaterialTheme.typography.bodySmall,
    )
    models?.let { found ->
        locality?.let { Text("This server is ${it.describe()}.") }
        for (model in found) {
            Row(
                modifier = Modifier.fillMaxWidth().clickable {
                    val chosen = LocalAiChoice(protocol, endpoint, model)
                    if (LocalAiPreference.save(context, chosen)) {
                        choice = chosen
                        message = "Chat will use $model."
                    } else {
                        message = "Could not save the choice."
                    }
                },
                verticalAlignment = Alignment.CenterVertically,
            ) {
                RadioButton(
                    selected = choice?.model == model && choice?.endpoint == endpoint,
                    onClick = null,
                )
                Text(model)
            }
        }
    }
    choice?.let { Text("Chat can use ${it.model} at ${it.endpoint}.") }
    message?.let { Text(it) }
}
