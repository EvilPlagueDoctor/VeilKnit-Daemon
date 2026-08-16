package com.example.veilknit_deamon

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update

private const val MAX_UI_LOG_LINES = 5_000

data class WalkSettingsUi(
    val normalMinHops: String = "5",
    val normalMaxHops: String = "100",
    val normalMinSeconds: String = "300",
    val normalTargetSeconds: String = "1800",
    val normalMaxSeconds: String = "7200",
    val mailMinHops: String = "7",
    val mailMaxHops: String = "135",
    val mailMinSeconds: String = "120",
    val mailTargetSeconds: String = "150",
    val mailMaxSeconds: String = "600",
    val automaticMailMode: Boolean = false,
)


data class PendingAppRequestUi(
    val requestId: Long,
    val appId: String,
    val displayName: String,
    val requestedAt: Long = 0,
    val expiresAt: Long = 0,
)

/**
 * An application registered on this daemon, as opposed to [FoundAppUi], which is an
 * application observed advertising itself on the network. Only the former can be rotated.
 */
data class LocalAppUi(
    val appId: String,
    val displayName: String,
    val enabled: Boolean = true,
    val credentialGeneration: Long = 0,
    val capabilityCount: Int = 0,
)

data class FoundAppUi(
    val appId: String,
    val observedHeaders: Int = 0,
    val discoveryCache: Int = 0,
    val recent: Int = 0,
    val archive: Int = 0,
    val totalVerifiedObservations: Long = 0,
)

data class NetworkSummaryUi(
    val sampledAt: Long = 0,
    val verified: Int = 0,
    val candidates: Int = 0,
    val authenticated: Int = 0,
    val online: Int = 0,
    val offline: Int = 0,
    val stale: Int = 0,
    val needsRefresh: Int = 0,
    val unknown: Int = 0,
    val presenceOk: Int = 0,
    val presenceFailed: Int = 0,
    val presenceUnread: Int = 0,
    val appHeaders: Int = 0,
    val mailboxCapable: Int = 0,
    val appSearches: Int = 0,
    val rootLookups: Int = 0,
    val walkState: String = "idle",
    val walkDone: Int = 0,
    val walkTotal: Int = 0,
    val walkNew: Int = 0,
    val walkUpdated: Int = 0,
    val walkReachable: Int = 0,
    val walkUnreachable: Int = 0,
)

data class DaemonUiState(
    val serviceRunning: Boolean = false,
    val nativeRunning: Boolean = false,
    val ready: Boolean = false,
    val authenticated: Boolean = false,
    val status: String = "Stopped",
    val mainDhtKey: String = "",
    val walkSettings: WalkSettingsUi = WalkSettingsUi(),
    val mainHeader: String = "Waiting for the first header read…",
    val mailboxHeader: String = "Waiting for the first header read…",
    val networkSummary: NetworkSummaryUi = NetworkSummaryUi(),
    val pendingAppRequests: List<PendingAppRequestUi> = emptyList(),
    val foundApps: List<FoundAppUi> = emptyList(),
    val localApps: List<LocalAppUi> = emptyList(),
    val logs: List<String> = emptyList(),
    val lastError: String? = null,
)

object DaemonStateStore {
    private val mutableState = MutableStateFlow(DaemonUiState())
    val state: StateFlow<DaemonUiState> = mutableState.asStateFlow()

    fun markServiceRunning(status: String = "Starting…") {
        mutableState.update {
            it.copy(
                serviceRunning = true,
                nativeRunning = false,
                ready = false,
                authenticated = false,
                status = status,
                mainDhtKey = "",
                walkSettings = WalkSettingsUi(),
                mainHeader = "Waiting for the first header read…",
                mailboxHeader = "Waiting for the first header read…",
                networkSummary = NetworkSummaryUi(),
                pendingAppRequests = emptyList(),
                foundApps = emptyList(),
                localApps = emptyList(),
                logs = emptyList(),
                lastError = null,
            )
        }
    }

    fun markNativeRunning(running: Boolean) {
        mutableState.update {
            it.copy(
                nativeRunning = running,
                serviceRunning = if (running) true else it.serviceRunning,
            )
        }
    }

    fun markStopped(message: String = "Stopped") {
        mutableState.update {
            it.copy(
                serviceRunning = false,
                nativeRunning = false,
                ready = false,
                authenticated = false,
                status = message,
            )
        }
    }

    fun setStatus(status: String) {
        mutableState.update { it.copy(status = status) }
    }

    fun setError(message: String) {
        mutableState.update { it.copy(status = "Error", lastError = message) }
    }

    fun appendLogs(lines: List<String>) {
        if (lines.isEmpty()) return
        mutableState.update { current ->
            var next = current
            for (line in lines) {
                val lower = line.lowercase()
                when {
                    "gui_app_requests_begin" in lower -> {
                        next = next.copy(pendingAppRequests = emptyList())
                    }
                    "gui_app_request=" in lower -> {
                        val marker = "GUI_APP_REQUEST="
                        val markerIndex = line.indexOf(marker, ignoreCase = true)
                        if (markerIndex >= 0) {
                            parsePendingAppRequest(line.substring(markerIndex + marker.length))?.let { request ->
                                next = next.copy(
                                    pendingAppRequests = (next.pendingAppRequests
                                        .filterNot { it.appId == request.appId } + request)
                                        .sortedBy { it.appId },
                                )
                            }
                        }
                    }
                    "gui_app_requests_end" in lower -> Unit
                    "gui_local_apps_begin" in lower -> {
                        next = next.copy(localApps = emptyList())
                    }
                    "gui_local_app=" in lower -> {
                        val marker = "GUI_LOCAL_APP="
                        val markerIndex = line.indexOf(marker, ignoreCase = true)
                        if (markerIndex >= 0) {
                            parseLocalApp(line.substring(markerIndex + marker.length))?.let { app ->
                                next = next.copy(
                                    localApps = (next.localApps.filterNot { it.appId == app.appId } + app)
                                        .sortedBy { it.appId },
                                )
                            }
                        }
                    }
                    "gui_local_apps_end" in lower -> Unit
                    "gui_apps_begin" in lower -> {
                        next = next.copy(foundApps = emptyList())
                    }
                    "gui_app=" in lower -> {
                        val marker = "GUI_APP="
                        val markerIndex = line.indexOf(marker, ignoreCase = true)
                        if (markerIndex >= 0) {
                            parseFoundApp(line.substring(markerIndex + marker.length))?.let { app ->
                                next = next.copy(foundApps = next.foundApps + app)
                            }
                        }
                    }
                    "gui_apps_end" in lower -> Unit
                    "gui_summary=" in lower -> {
                        val marker = "GUI_SUMMARY="
                        val markerIndex = line.indexOf(marker, ignoreCase = true)
                        if (markerIndex >= 0) {
                            parseNetworkSummary(line.substring(markerIndex + marker.length))?.let { summary ->
                                next = next.copy(networkSummary = summary)
                            }
                        }
                    }
                    "main_dht_key=" in lower -> {
                        val marker = "MAIN_DHT_KEY="
                        val markerIndex = line.indexOf(marker, ignoreCase = true)
                        if (markerIndex >= 0) {
                            next = next.copy(
                                mainDhtKey = line.substring(markerIndex + marker.length).trim(),
                            )
                        }
                    }
                    "walk_settings=" in lower -> {
                        val marker = "WALK_SETTINGS="
                        val markerIndex = line.indexOf(marker, ignoreCase = true)
                        val values = if (markerIndex >= 0) {
                            line.substring(markerIndex + marker.length).trim().split(',')
                        } else emptyList()
                        if (values.size == 11) {
                            next = next.copy(
                                walkSettings = WalkSettingsUi(
                                    normalMinHops = values[0],
                                    normalMaxHops = values[1],
                                    normalMinSeconds = values[2],
                                    normalTargetSeconds = values[3],
                                    normalMaxSeconds = values[4],
                                    mailMinHops = values[5],
                                    mailMaxHops = values[6],
                                    mailMinSeconds = values[7],
                                    mailTargetSeconds = values[8],
                                    mailMaxSeconds = values[9],
                                    automaticMailMode = values[10] == "1" || values[10].equals("true", true),
                                ),
                            )
                        }
                    }
                    "main_header=" in lower -> {
                        val marker = "MAIN_HEADER="
                        val markerIndex = line.indexOf(marker, ignoreCase = true)
                        if (markerIndex >= 0) {
                            next = next.copy(
                                mainHeader = decodeGuiValue(line.substring(markerIndex + marker.length)),
                            )
                        }
                    }
                    "mailbox_header=" in lower -> {
                        val marker = "MAILBOX_HEADER="
                        val markerIndex = line.indexOf(marker, ignoreCase = true)
                        if (markerIndex >= 0) {
                            next = next.copy(
                                mailboxHeader = decodeGuiValue(line.substring(markerIndex + marker.length)),
                            )
                        }
                    }
                    "[gui] ready" in lower -> {
                        next = next.copy(
                            ready = true,
                            authenticated = true,
                            status = "Running",
                            lastError = null,
                        )
                    }
                    "welcome," in lower -> {
                        next = next.copy(
                            authenticated = true,
                            status = "Authenticated; starting network services…",
                            lastError = null,
                        )
                    }
                    "wrong password" in lower ||
                        "no account with that username" in lower ||
                        "username is already taken" in lower ||
                        "usernames may only contain" in lower -> {
                        next = next.copy(
                            status = "Authentication failed",
                            lastError = line.substringAfter("] ", line),
                        )
                    }
                    "daemon error:" in lower || "panicked" in lower -> {
                        next = next.copy(
                            status = "Error",
                            lastError = line.substringAfter("] ", line),
                        )
                    }
                    "network services stopped safely" in lower ||
                        "[android] daemon stopped" in lower -> {
                        next = next.copy(
                            ready = false,
                            nativeRunning = false,
                            status = "Stopped",
                        )
                    }
                }
            }

            val visibleLines = lines.filterNot { line ->
                line.contains("GUI_SUMMARY=", ignoreCase = true) ||
                    line.contains("GUI_APP_REQUESTS_BEGIN", ignoreCase = true) ||
                    line.contains("GUI_APP_REQUEST=", ignoreCase = true) ||
                    line.contains("GUI_APP_REQUESTS_END", ignoreCase = true) ||
                    line.contains("GUI_APPS_BEGIN", ignoreCase = true) ||
                    line.contains("GUI_APP=", ignoreCase = true) ||
                    line.contains("GUI_APPS_END", ignoreCase = true)
            }
            val combined = if (current.logs.size + visibleLines.size <= MAX_UI_LOG_LINES) {
                current.logs + visibleLines
            } else {
                (current.logs + visibleLines).takeLast(MAX_UI_LOG_LINES)
            }
            next.copy(logs = combined)
        }
    }
}


private fun guiFields(value: String): Map<String, String> = value.split(';')
    .mapNotNull { part ->
        val split = part.indexOf('=')
        if (split <= 0) null else part.substring(0, split) to part.substring(split + 1)
    }
    .toMap()

private fun decodeHexUtf8(value: String): String? {
    if (value.length % 2 != 0) return null
    return try {
        val bytes = ByteArray(value.length / 2)
        for (index in bytes.indices) {
            val high = value[index * 2].digitToInt(16)
            val low = value[index * 2 + 1].digitToInt(16)
            bytes[index] = ((high shl 4) or low).toByte()
        }
        bytes.toString(Charsets.UTF_8)
    } catch (_: IllegalArgumentException) {
        null
    }
}

private fun parsePendingAppRequest(value: String): PendingAppRequestUi? {
    val fields = guiFields(value)
    val requestId = fields["request_id"]?.toLongOrNull() ?: return null
    val appId = fields["app_hex"]?.let(::decodeHexUtf8)?.takeIf { it.isNotBlank() } ?: return null
    val displayName = fields["name_hex"]?.let(::decodeHexUtf8) ?: ""
    return PendingAppRequestUi(
        requestId = requestId,
        appId = appId,
        displayName = displayName,
        requestedAt = fields["requested_at"]?.toLongOrNull() ?: 0L,
        expiresAt = fields["expires_at"]?.toLongOrNull() ?: 0L,
    )
}

private fun parseLocalApp(value: String): LocalAppUi? {
    val fields = guiFields(value)
    val appId = fields["app_hex"]?.let(::decodeHexUtf8)?.takeIf { it.isNotBlank() } ?: return null
    return LocalAppUi(
        appId = appId,
        displayName = fields["name_hex"]?.let(::decodeHexUtf8).orEmpty(),
        enabled = fields["enabled"] != "0",
        credentialGeneration = fields["generation"]?.toLongOrNull() ?: 0L,
        capabilityCount = fields["capabilities"]?.toIntOrNull() ?: 0,
    )
}

private fun parseFoundApp(value: String): FoundAppUi? {
    val fields = guiFields(value)
    val appId = fields["app_hex"]?.let(::decodeHexUtf8)?.takeIf { it.isNotBlank() } ?: return null
    return FoundAppUi(
        appId = appId,
        observedHeaders = fields["observed"]?.toIntOrNull() ?: 0,
        discoveryCache = fields["cached"]?.toIntOrNull() ?: 0,
        recent = fields["recent"]?.toIntOrNull() ?: 0,
        archive = fields["archive"]?.toIntOrNull() ?: 0,
        totalVerifiedObservations = fields["observations"]?.toLongOrNull() ?: 0L,
    )
}

private fun parseNetworkSummary(value: String): NetworkSummaryUi? {
    val fields = value.split(';')
        .mapNotNull { part ->
            val split = part.indexOf('=')
            if (split <= 0) null else part.substring(0, split) to part.substring(split + 1)
        }
        .toMap()
    if (fields.isEmpty()) return null
    fun int(name: String): Int = fields[name]?.toIntOrNull() ?: 0
    return NetworkSummaryUi(
        sampledAt = fields["sampled_at"]?.toLongOrNull() ?: 0L,
        verified = int("verified"),
        candidates = int("candidates"),
        authenticated = int("authenticated"),
        online = int("online"),
        offline = int("offline"),
        stale = int("stale"),
        needsRefresh = int("refresh"),
        unknown = int("unknown"),
        presenceOk = int("presence_ok"),
        presenceFailed = int("presence_failed"),
        presenceUnread = int("presence_unread"),
        appHeaders = int("app_headers"),
        mailboxCapable = int("mailbox_capable"),
        appSearches = int("app_searches"),
        rootLookups = int("root_lookups"),
        walkState = fields["walk_state"] ?: "idle",
        walkDone = int("walk_done"),
        walkTotal = int("walk_total"),
        walkNew = int("walk_new"),
        walkUpdated = int("walk_updated"),
        walkReachable = int("walk_reachable"),
        walkUnreachable = int("walk_unreachable"),
    )
}

private fun decodeGuiValue(value: String): String {
    val decoded = StringBuilder(value.length)
    var escaped = false
    for (character in value) {
        if (!escaped) {
            if (character == '\\') escaped = true else decoded.append(character)
            continue
        }
        when (character) {
            'n' -> decoded.append('\n')
            'r' -> Unit
            't' -> decoded.append('\t')
            '\\' -> decoded.append('\\')
            else -> decoded.append('\\').append(character)
        }
        escaped = false
    }
    if (escaped) decoded.append('\\')
    return decoded.toString()
}

enum class LogCategory {
    Overview,
    Handshake,
    Network,
    Headers,
    Dht,
    Mailbox,
    Applications,
    All,
}

fun List<String>.forCategory(category: LogCategory): List<String> {
    if (category == LogCategory.All) return this
    return filter { line ->
        val lower = line.lowercase()
        when (category) {
            LogCategory.Overview -> listOf(
                "welcome", "ready", "startup", "shutdown", "authenticated", "main_dht_key",
                "network services", "daemon error", "warning",
            ).any { token -> lower.contains(token) }
            LogCategory.Handshake -> "handshake" in lower || "vld0" in lower
            LogCategory.Network -> listOf(
                "network", "route", "walk", "node", "veilid", "attachment", "presence",
                "needs refresh", "stale online claim",
            ).any { token -> lower.contains(token) }
            LogCategory.Headers -> listOf(
                "header", "presence subkey", "mailbox advertisement", "main_header=", "mailbox_header=",
            ).any { token -> lower.contains(token) }
            LogCategory.Dht -> listOf("dht", "record key", "subkey").any { token -> lower.contains(token) }
            LogCategory.Mailbox -> listOf("mail", "custodian", "inbox", "outgoing").any { token -> lower.contains(token) }
            LogCategory.Applications -> listOf(
                "application", "app-", "registration", "credential", "api",
            ).any { token -> lower.contains(token) }
            LogCategory.All -> true
        }
    }
}
