package com.example.veilknit_deamon

import java.io.File
import java.util.concurrent.atomic.AtomicReference

/** Runtime information used by the exported, signature-protected Binder proxy. */
object DaemonApiRuntime {
    private val username = AtomicReference("")

    fun setUsername(value: String) {
        username.set(value)
    }

    fun clearUsername() {
        username.set("")
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
