package org.foundation.wikipedia

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.unit.dp
import uniffi.wiki_ffi.PackTrust
import uniffi.wiki_ffi.packTrustDescription
import uniffi.wiki_ffi.packTrustLabel

/** Glyph shown beside the label; each status has its own shape, not only a colour. */
internal fun packTrustGlyph(trust: PackTrust): String = when (trust) {
    PackTrust.VERIFIED_PUBLISHER -> "✔"
    PackTrust.CUSTOM_INTEGRITY_VERIFIED -> "◇"
    PackTrust.NOT_YET_CHECKED -> "?"
    PackTrust.VERIFICATION_FAILED -> "!"
}

/** Test tag of a badge, so tests can find the status a surface shows. */
fun packTrustTag(trust: PackTrust): String = "pack-trust-${trust.name}"

@Composable
private fun trustColors(trust: PackTrust): Pair<Color, Color> = when (trust) {
    PackTrust.VERIFIED_PUBLISHER ->
        MaterialTheme.colorScheme.primary to MaterialTheme.colorScheme.onPrimary
    PackTrust.CUSTOM_INTEGRITY_VERIFIED ->
        MaterialTheme.colorScheme.secondaryContainer to MaterialTheme.colorScheme.onSecondaryContainer
    PackTrust.NOT_YET_CHECKED ->
        MaterialTheme.colorScheme.surfaceVariant to MaterialTheme.colorScheme.onSurfaceVariant
    PackTrust.VERIFICATION_FAILED ->
        MaterialTheme.colorScheme.error to MaterialTheme.colorScheme.onError
}

/**
 * The trust status of a pack (TRUST-002), with the shared Rust label and
 * plain-language description. Screen readers hear "Trust status: <label>.
 * <description>" as one element. Used on library cards, the import status
 * and the install confirmation, so every surface shows the same text.
 */
@Composable
fun PackTrustBadge(
    trust: PackTrust,
    modifier: Modifier = Modifier,
    showDescription: Boolean = true,
) {
    val label = packTrustLabel(trust)
    val description = packTrustDescription(trust)
    val (container, content) = trustColors(trust)
    Row(
        modifier = modifier
            .testTag(packTrustTag(trust))
            .clearAndSetSemantics {
                contentDescription = "Trust status: $label. $description"
            },
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.Top,
    ) {
        Surface(color = container, contentColor = content, shape = CircleShape) {
            Box(Modifier.size(24.dp), contentAlignment = Alignment.Center) {
                Text(packTrustGlyph(trust), style = MaterialTheme.typography.labelLarge)
            }
        }
        Column {
            Text(label, style = MaterialTheme.typography.titleSmall)
            if (showDescription) {
                Text(description, style = MaterialTheme.typography.bodySmall)
            }
        }
    }
}
