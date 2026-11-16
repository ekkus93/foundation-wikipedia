package org.foundation.wikipedia

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import uniffi.wiki_ffi.LocalAiProtocol

class LocalAiProfilesTest {
    private fun choice(n: Int, protocol: LocalAiProtocol = LocalAiProtocol.OLLAMA) =
        LocalAiChoice(protocol, "http://192.168.1.$n:11434", "model-$n")

    @Test
    fun removingTheActiveProfileNeverActivatesAnother() {
        val a = choice(1)
        val b = choice(2)
        val profiles = LocalAiProfiles().saveAndActivate(a)!!.saveAndActivate(b)!!
        assertEquals(b, profiles.active)
        val removed = profiles.remove(b)
        assertNull(removed.active)
        assertEquals(listOf(a), removed.profiles)
        // Removing an inactive profile keeps the active choice.
        assertEquals(b, profiles.activate(b).remove(a).active)
    }

    @Test
    fun disablingKeepsProfilesAndActivatingNeedsASavedOne() {
        val a = choice(1)
        val profiles = LocalAiProfiles().saveAndActivate(a)!!.deactivate()
        assertNull(profiles.active)
        assertEquals(listOf(a), profiles.profiles)
        assertEquals(a, profiles.activate(a).active)
        assertEquals(profiles, profiles.activate(choice(9)))
    }

    @Test
    fun atMostEightAreSavedAndDuplicatesAreNotAdded() {
        var profiles = LocalAiProfiles()
        for (n in 1..LocalAiProfiles.MAX) profiles = profiles.saveAndActivate(choice(n))!!
        assertNull(profiles.saveAndActivate(choice(99)))
        assertEquals(LocalAiProfiles.MAX, profiles.saveAndActivate(choice(1))!!.profiles.size)
    }

    @Test
    fun encodingRoundTripsAndMalformedTextFailsClosed() {
        val a = choice(1)
        val b = choice(2, LocalAiProtocol.OPEN_AI_COMPATIBLE)
        val profiles = LocalAiProfiles().saveAndActivate(a)!!.saveAndActivate(b)!!.activate(a)
        assertEquals(profiles, LocalAiProfiles.decode(profiles.encode()))
        assertEquals(LocalAiProfiles(), LocalAiProfiles.decode(LocalAiProfiles().encode()))
        val good = profiles.encode()
        for (bad in listOf(
            "",
            "local-ai-profiles/v2",
            good.replace("-OPEN_AI_COMPATIBLE", "*OPEN_AI_COMPATIBLE"), // two active
            good + "\n" + good.lines()[1], // duplicate
            good.replace("OLLAMA", "CLOUD"),
            good.replace("\tmodel-1", "\t"),
            good + "\n-OLLAMA\thttp://x\tm\textra",
        )) {
            assertNull(bad, LocalAiProfiles.decode(bad))
        }
        assertNull(LocalAiProfiles.decode(null))
    }

    @Test
    fun controlCharactersCannotBeStored() {
        assertEquals(false, LocalAiProfiles.isStorable(LocalAiChoice(LocalAiProtocol.OLLAMA, "http://1.2.3.4", "a\tb")))
        assertEquals(true, LocalAiProfiles.isStorable(choice(1)))
    }
}
