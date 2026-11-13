package org.foundation.wikipedia

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Text
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
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import org.foundation.wikipedia.rust.HostedAiKeyMissingException
import org.foundation.wikipedia.rust.HostedAiKeyStorageException
import org.foundation.wikipedia.rust.RustHostedAi
import org.foundation.wikipedia.rust.host
import uniffi.wiki_ffi.HostedAiException
import uniffi.wiki_ffi.HostedAiProvider

/** User-facing text for a hosted-AI failure. Never includes key material. */
fun hostedAiErrorMessage(error: Throwable): String = when (error) {
    is HostedAiKeyMissingException -> "Save an API key for this provider first."
    is HostedAiKeyStorageException -> error.message ?: "Secure key storage failed."
    is IllegalArgumentException -> "That does not look like a valid API key."
    is HostedAiException.InvalidKey -> "That does not look like a valid API key."
    is HostedAiException.InvalidRequest -> "Enter a valid model name."
    is HostedAiException.NotReady ->
        "Secure connections are not available on this device yet."
    is HostedAiException.Rejected ->
        "The provider refused the request. Check the API key and model name."
    is HostedAiException.Timeout -> "The provider did not respond in time."
    is HostedAiException.Network ->
        "Could not connect securely. Check the network connection."
    is HostedAiException.InvalidResponse -> "The provider sent an unexpected response."
    is LinkageError -> "Hosted AI is not available in this build."
    else -> "Something went wrong with the hosted AI provider."
}

private fun HostedAiProvider.title() = when (this) {
    HostedAiProvider.ANTHROPIC -> "Anthropic"
    HostedAiProvider.OPEN_AI -> "OpenAI"
}

/**
 * Settings › AI Assistant: save, replace or delete a hosted provider's API key
 * (Android Keystore-encrypted, never shown again) and run an explicit,
 * disclosed connection check through the shared Rust HTTPS transport.
 */
@Composable
fun HostedAiSection() {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val hosted = remember { RustHostedAi(context) }
    var provider by remember { mutableStateOf(HostedAiProvider.ANTHROPIC) }
    var configured by remember { mutableStateOf<Boolean?>(null) }
    var keyInput by remember { mutableStateOf("") }
    var model by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var message by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(provider) {
        configured = try {
            hosted.isKeyConfigured(provider)
        } catch (error: Exception) {
            message = hostedAiErrorMessage(error)
            null
        }
    }

    fun perform(action: suspend () -> String) {
        scope.launch {
            busy = true
            message = try {
                action()
            } catch (error: Exception) {
                hostedAiErrorMessage(error)
            } catch (error: LinkageError) {
                hostedAiErrorMessage(error)
            } finally {
                busy = false
            }
        }
    }

    Text("Hosted AI provider", style = MaterialTheme.typography.titleMedium)
    Text(
        "Optional. Hosted models run on the provider's servers: questions and article " +
            "excerpts you send leave this device. Keys are encrypted with the Android " +
            "Keystore and are never shown again or included in backups.",
        style = MaterialTheme.typography.bodySmall,
    )
    for (option in HostedAiProvider.entries) {
        Row(
            modifier = Modifier.fillMaxWidth().clickable(enabled = !busy) {
                provider = option
                message = null
            },
            verticalAlignment = Alignment.CenterVertically,
        ) {
            RadioButton(selected = provider == option, onClick = null)
            Text(option.title())
        }
    }
    Text(
        when (configured) {
            true -> "An API key is saved for ${provider.title()}."
            false -> "No API key is saved for ${provider.title()}."
            null -> "Checking saved key…"
        },
    )
    OutlinedTextField(
        value = keyInput,
        onValueChange = { keyInput = it },
        label = { Text("${provider.title()} API key") },
        singleLine = true,
        visualTransformation = PasswordVisualTransformation(),
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password, autoCorrect = false),
        modifier = Modifier.fillMaxWidth(),
    )
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Button(enabled = !busy && keyInput.isNotBlank(), onClick = {
            val entered = keyInput
            perform {
                hosted.saveKey(provider, entered)
                keyInput = ""
                configured = true
                "Saved the ${provider.title()} key."
            }
        }) { Text(if (configured == true) "Replace key" else "Save key") }
        if (configured == true) {
            OutlinedButton(enabled = !busy, onClick = {
                perform {
                    hosted.deleteKey(provider)
                    configured = false
                    "Deleted the ${provider.title()} key."
                }
            }) { Text("Delete key") }
        }
    }
    OutlinedTextField(
        value = model,
        onValueChange = { model = it },
        label = { Text("Model name") },
        singleLine = true,
        modifier = Modifier.fillMaxWidth(),
    )
    OutlinedButton(
        enabled = !busy && configured == true && model.isNotBlank(),
        onClick = {
            val chosen = model
            perform {
                val check = hosted.checkConnection(provider, chosen)
                "Connected. The model replied: “${check.reply.take(80)}”"
            }
        },
        modifier = Modifier.fillMaxWidth(),
    ) { Text("Test connection") }
    Text(
        "Test connection sends a short test message and your key to ${provider.host}.",
        style = MaterialTheme.typography.bodySmall,
    )
    message?.let { Text(it) }
}
