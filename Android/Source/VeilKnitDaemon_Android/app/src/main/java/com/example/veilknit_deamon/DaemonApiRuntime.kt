package com.example.veilknit_deamon

import java.io.File
import java.util.UUID
import java.util.concurrent.atomic.AtomicReference
import org.json.JSONObject

/** Runtime information used by the exported, signature-protected Binder proxy. */
object DaemonApiRuntime {
    private val username = AtomicReference("")
    private val daemonInstanceId = AtomicReference("")

    fun setUsername(value: String) {
        val previous = username.getAndSet(value)
        if (previous != value || daemonInstanceId.get().isBlank()) {
            daemonInstanceId.set(UUID.randomUUID().toString())
        }
    }

    fun clearUsername() {
        username.set("")
        daemonInstanceId.set("")
    }

    /**
     * Unique for one foreground/native daemon run. Client apps should treat a change here as
     * invalidating every in-memory/session token, even when the same account signs back in.
     */
    fun daemonInstanceId(): String = daemonInstanceId.get()

    /**
     * Read the active network profile written by the Rust local API. The username check is
     * important during account switching: an old daemon_endpoint.json can remain on disk until
     * the new account reaches API startup, and must not be mistaken for the new account.
     */
    fun activeProfileId(filesDir: File): String {
        val currentUser = username.get().trim()
        if (currentUser.isEmpty()) return ""
        val discovery = File(filesDir, "app_credentials/daemon_endpoint.json")
        if (!discovery.exists()) return ""
        return runCatching {
            val json = JSONObject(discovery.readText(Charsets.UTF_8))
            if (json.optString("username").trim() != currentUser) ""
            else json.optString("profile_id").trim()
        }.getOrDefault("")
    }

    fun endpointFile(filesDir: File): File? {
        val current = username.get().trim()
        if (current.isEmpty()) return null
        val safe = current.map { character ->
            if (character.isLetterOrDigit() || character == '-' || character == '_') character else '_'
        }.joinToString("")
        return File(filesDir, "veilid-network-$safe.sock")
    }
}
