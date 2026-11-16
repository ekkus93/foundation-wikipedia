package org.foundation.wikipedia

/** The device's current network, as far as downloads are concerned. */
enum class DownloadNetwork { NONE, UNMETERED, METERED }

/** Why an official catalog check or pack download may or may not start. */
sealed interface DownloadGate {
    data object Allowed : DownloadGate

    /** The user has not yet accepted the GitHub privacy disclosure. */
    data object NeedsDisclosure : DownloadGate

    /** The reader is in Offline mode; nothing may use the network. */
    data object WorkingOffline : DownloadGate

    data object NoNetwork : DownloadGate

    /** Only mobile data is available and the user allowed Wi-Fi only. */
    data object WifiOnly : DownloadGate

    data class NotEnoughSpace(val neededBytes: Long, val availableBytes: Long) : DownloadGate
}

/**
 * MOBPACK-001 download policy, checked before any official-pack network
 * request: privacy disclosure first, then Offline mode, connectivity and
 * the Wi-Fi-only setting, then (for a pack) free space.
 */
fun officialDownloadGate(
    disclosureAccepted: Boolean,
    workingOffline: Boolean,
    network: DownloadNetwork,
    allowMobileData: Boolean,
    neededBytes: Long? = null,
    availableBytes: Long? = null,
): DownloadGate = when {
    !disclosureAccepted -> DownloadGate.NeedsDisclosure
    workingOffline -> DownloadGate.WorkingOffline
    network == DownloadNetwork.NONE -> DownloadGate.NoNetwork
    network == DownloadNetwork.METERED && !allowMobileData -> DownloadGate.WifiOnly
    neededBytes != null && availableBytes != null && neededBytes > availableBytes ->
        DownloadGate.NotEnoughSpace(neededBytes, availableBytes)
    else -> DownloadGate.Allowed
}

/** Plain-language reason a blocked download did not start. */
fun downloadGateMessage(gate: DownloadGate): String? = when (gate) {
    DownloadGate.Allowed, DownloadGate.NeedsDisclosure -> null
    DownloadGate.WorkingOffline -> "Offline mode is on. Go online to check for or download packs."
    DownloadGate.NoNetwork -> "No internet connection."
    DownloadGate.WifiOnly ->
        "Connect to Wi-Fi, or allow downloads over mobile data, to continue."
    is DownloadGate.NotEnoughSpace ->
        "Not enough free space: this needs ${formatStorage(gate.neededBytes)}, " +
            "but only ${formatStorage(gate.availableBytes)} is free."
}

/** Human-readable storage size (1 decimal for MB and GB). */
fun formatStorage(bytes: Long): String {
    val mib = bytes / (1024.0 * 1024.0)
    return when {
        mib >= 1024 -> "%.1f GB".format(mib / 1024)
        mib >= 1 -> "%.1f MB".format(mib)
        else -> "$bytes bytes"
    }
}

/** Persisted official-pack download choices (both off until the user acts). */
internal object OfficialPackPreference {
    const val DISCLOSURE_KEY = "official_packs_disclosure"
    const val MOBILE_DATA_KEY = "official_packs_mobile_data"
    private const val ACCEPTED = "v1:accepted"
    private const val ALLOWED = "v1:allowed"

    fun disclosureAccepted(value: String?): Boolean = value == ACCEPTED
    fun encodeDisclosure(): String = ACCEPTED

    fun mobileDataAllowed(value: String?): Boolean = value == ALLOWED
    fun encodeMobileData(allowed: Boolean): String = if (allowed) ALLOWED else "v1:wifi-only"
}

/** Shown before the first network request for official packs (spec §21, ADR 0009). */
const val OFFICIAL_PACK_DISCLOSURE =
    "Official packs are listed and downloaded from GitHub (github.com and " +
        "githubusercontent.com). GitHub sees your IP address and which files you download. " +
        "Nothing else about you or your reading is sent. Every pack is checked against the " +
        "publisher's signature before it is installed."
