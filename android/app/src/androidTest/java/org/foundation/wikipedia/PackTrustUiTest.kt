package org.foundation.wikipedia

import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.Column
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.accessibility.enableAccessibilityChecks
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.tryPerformAccessibilityChecks
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.wiki_ffi.PackReview
import uniffi.wiki_ffi.PackTrust
import uniffi.wiki_ffi.packTrustDescription
import uniffi.wiki_ffi.packTrustLabel

/**
 * TRUST-002: every trust status has its own visible label, glyph and spoken
 * description (shared from Rust), and the install confirmation offers
 * "Install" only for a verified pack.
 */
@OptIn(ExperimentalTestApi::class)
@RunWith(AndroidJUnit4::class)
class PackTrustUiTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private fun spoken(trust: PackTrust): String =
        compose.onNodeWithTag(packTrustTag(trust)).fetchSemanticsNode()
            .config[SemanticsProperties.ContentDescription].single()

    @Test
    fun eachStatusIsDistinctVisibleAndAccessible() {
        compose.setContent {
            Column { for (trust in PackTrust.entries) PackTrustBadge(trust) }
        }
        val spokenTexts = PackTrust.entries.map { trust ->
            compose.onNodeWithTag(packTrustTag(trust)).assertIsDisplayed()
            val label = packTrustLabel(trust)
            assertEquals(
                "Trust status: $label. ${packTrustDescription(trust)}",
                spoken(trust),
            )
            label
        }
        assertEquals(
            listOf(
                "Verified publisher",
                "Custom — integrity verified",
                "Not yet checked",
                "Verification failed",
            ),
            spokenTexts,
        )
        assertEquals(4, PackTrust.entries.map(::packTrustGlyph).toSet().size)
        compose.enableAccessibilityChecks()
        compose.onRoot().tryPerformAccessibilityChecks()
    }

    private fun review(trust: PackTrust) = PackReview(
        snapshotId = "moon-pack",
        project = "enwiki",
        trust = trust,
        trustLabel = packTrustLabel(trust),
        articles = 2uL,
        declaredBytes = 4096uL,
    )

    @Test
    fun onlyAVerifiedPackCanBeConfirmed() {
        var trust by mutableStateOf(PackTrust.CUSTOM_INTEGRITY_VERIFIED)
        val decisions = mutableListOf<Boolean>()
        compose.setContent { PackReviewDialog(review(trust)) { decisions += it } }

        compose.onNodeWithText("Install custom pack?").assertIsDisplayed()
        compose.onNodeWithTag(packTrustTag(PackTrust.CUSTOM_INTEGRITY_VERIFIED)).assertIsDisplayed()
        assertFalse(
            "a custom pack never claims a publisher",
            compose.onAllNodesWithText("Verified publisher", substring = true)
                .fetchSemanticsNodes().isNotEmpty(),
        )
        compose.enableAccessibilityChecks()
        compose.onRoot().tryPerformAccessibilityChecks()
        compose.onNodeWithText("Install").performClick()
        assertEquals(listOf(true), decisions)

        for (unverified in listOf(PackTrust.NOT_YET_CHECKED, PackTrust.VERIFICATION_FAILED)) {
            trust = unverified
            compose.waitForIdle()
            compose.onNodeWithText("This pack cannot be installed").assertIsDisplayed()
            compose.onNodeWithTag(packTrustTag(unverified)).assertIsDisplayed()
            assertTrue(
                compose.onAllNodesWithText("Install").fetchSemanticsNodes().isEmpty(),
            )
        }
        compose.onNodeWithText("Cancel").performClick()
        assertEquals(listOf(true, false), decisions)
    }
}
