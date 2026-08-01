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

            val combined = if (current.logs.size + lines.size <= MAX_UI_LOG_LINES) {
                current.logs + lines
            } else {
                (current.logs + lines).takeLast(MAX_UI_LOG_LINES)
            }
            next.copy(logs = combined)
        }
    }
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
