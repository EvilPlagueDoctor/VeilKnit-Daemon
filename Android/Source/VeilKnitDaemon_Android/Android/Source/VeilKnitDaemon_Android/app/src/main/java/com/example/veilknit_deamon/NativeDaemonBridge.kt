package com.example.veilknit_deamon

import org.json.JSONArray
import android.content.Context

object NativeDaemonBridge {
    val isLibraryLoaded: Boolean
    val loadError: String?

    init {
        val result = runCatching { System.loadLibrary("veilknit_daemon") }
        isLibraryLoaded = result.isSuccess
        loadError = result.exceptionOrNull()?.message
    }

    @JvmStatic
    private external fun nativeStart(
        context: Context,
        dataDirectory: String,
        signup: Boolean,
        username: String,
        password: String,
    ): Boolean

    @JvmStatic
    private external fun nativeSendCommand(command: String): Boolean

    @JvmStatic
    private external fun nativeRequestStop(): Boolean

    @JvmStatic
    private external fun nativeIsRunning(): Boolean

    @JvmStatic
    private external fun nativeDrainLogs(): String

    fun start(
        context: Context,
        dataDirectory: String,
        signup: Boolean,
        username: String,
        password: String,
    ): Boolean = isLibraryLoaded && runCatching {
        nativeStart(
            context.applicationContext,
            dataDirectory,
            signup,
            username,
            password,
        )
    }.getOrDefault(false)

    fun sendCommand(command: String): Boolean = isLibraryLoaded && runCatching {
        nativeSendCommand(command)
    }.getOrDefault(false)

    fun requestStop(): Boolean = isLibraryLoaded && runCatching {
        nativeRequestStop()
    }.getOrDefault(false)

    fun isRunning(): Boolean = isLibraryLoaded && runCatching {
        nativeIsRunning()
    }.getOrDefault(false)

    fun drainLogs(): List<String> {
        if (!isLibraryLoaded) return emptyList()
        val raw = runCatching { nativeDrainLogs() }.getOrDefault("[]")
        return runCatching {
            val array = JSONArray(raw)
            buildList(array.length()) {
                for (index in 0 until array.length()) {
                    add(array.optString(index))
                }
            }
        }.getOrDefault(emptyList())
    }
}
