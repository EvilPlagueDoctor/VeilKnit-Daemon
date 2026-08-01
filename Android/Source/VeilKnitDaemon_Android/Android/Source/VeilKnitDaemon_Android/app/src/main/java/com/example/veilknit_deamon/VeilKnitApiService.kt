package com.example.veilknit_deamon

import android.app.Service
import android.content.Intent
import android.content.pm.PackageManager
import android.net.LocalSocket
import android.net.LocalSocketAddress
import android.os.Binder
import android.os.IBinder
import android.os.RemoteException
import android.os.Process
import com.example.veilknit_deamon.ipc.IVeilKnitApi
import com.example.veilknit_deamon.ipc.IVeilKnitStreamCallback
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import org.json.JSONObject
import java.io.BufferedReader
import java.io.BufferedWriter
import java.io.File
import java.io.InputStreamReader
import java.io.OutputStreamWriter
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicLong

/**
 * Signature-protected bridge between Android applications and the daemon's
 * existing protocol-v3 Unix-domain socket. No network secret or writer key is
 * exposed by this service; it forwards the same JSON lines used on desktop.
 */
class VeilKnitApiService : Service() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val nextSubscriptionId = AtomicLong(1)
    private val subscriptions = ConcurrentHashMap<Long, Subscription>()

    override fun onBind(intent: Intent?): IBinder = binder

    override fun onDestroy() {
        subscriptions.values.forEach { it.close() }
        subscriptions.clear()
        scope.cancel()
        super.onDestroy()
    }

    private val binder = object : IVeilKnitApi.Stub() {
        override fun getDaemonStateJson(): String {
            verifyCaller()
            val state = DaemonStateStore.state.value
            return JSONObject()
                .put("service_running", state.serviceRunning)
                .put("native_running", state.nativeRunning)
                .put("ready", state.ready)
                .put("authenticated", state.authenticated)
                .put("status", state.status)
                .put("main_dht_key", state.mainDhtKey)
                .put("last_error", state.lastError ?: JSONObject.NULL)
                .toString()
        }

        override fun transact(requestJson: String): String {
            verifyCaller()
            require(requestJson.toByteArray(Charsets.UTF_8).size <= MAX_REQUEST_BYTES) {
                "Request exceeds $MAX_REQUEST_BYTES bytes"
            }
            return transactLine(requestJson)
        }

        override fun subscribe(requestJson: String, callback: IVeilKnitStreamCallback): Long {
            verifyCaller()
            require(requestJson.toByteArray(Charsets.UTF_8).size <= MAX_REQUEST_BYTES) {
                "Request exceeds $MAX_REQUEST_BYTES bytes"
            }
            val id = nextSubscriptionId.getAndIncrement()
            val subscription = Subscription(id, callback)
            subscriptions[id] = subscription
            subscription.job = scope.launch { runSubscription(subscription, requestJson) }
            return id
        }

        override fun unsubscribe(subscriptionId: Long) {
            verifyCaller()
            subscriptions.remove(subscriptionId)?.close()
        }
    }

    private fun verifyCaller() {
        if (Binder.getCallingUid() == Process.myUid()) return
        if (checkCallingPermission(PERMISSION) != PackageManager.PERMISSION_GRANTED) {
            throw SecurityException("Calling application is not signed for the VeilKnit local API")
        }
    }

    private fun transactLine(requestJson: String): String {
        val endpoint = awaitEndpoint()
        LocalSocket().use { socket ->
            socket.connect(
                LocalSocketAddress(endpoint.absolutePath, LocalSocketAddress.Namespace.FILESYSTEM)
            )
            socket.soTimeout = RESPONSE_TIMEOUT_MS
            BufferedWriter(OutputStreamWriter(socket.outputStream, Charsets.UTF_8)).use { writer ->
                BufferedReader(InputStreamReader(socket.inputStream, Charsets.UTF_8)).use { reader ->
                    writer.write(requestJson)
                    writer.newLine()
                    writer.flush()
                    return reader.readLine() ?: throw IllegalStateException("Daemon API closed without a response")
                }
            }
        }
    }

    private suspend fun runSubscription(subscription: Subscription, requestJson: String) {
        try {
            val endpoint = awaitEndpoint()
            val socket = LocalSocket()
            subscription.socket = socket
            socket.connect(
                LocalSocketAddress(endpoint.absolutePath, LocalSocketAddress.Namespace.FILESYSTEM)
            )
            val writer = BufferedWriter(OutputStreamWriter(socket.outputStream, Charsets.UTF_8))
            val reader = BufferedReader(InputStreamReader(socket.inputStream, Charsets.UTF_8))
            writer.write(requestJson)
            writer.newLine()
            writer.flush()
            while (!subscription.closed) {
                val line = reader.readLine() ?: break
                try {
                    subscription.callback.onLine(line)
                } catch (_: RemoteException) {
                    break
                }
            }
            notifyClosed(subscription, "Daemon stream closed")
        } catch (error: Throwable) {
            notifyClosed(subscription, error.message ?: error.javaClass.simpleName)
        } finally {
            subscriptions.remove(subscription.id)
            subscription.close()
        }
    }

    private fun notifyClosed(subscription: Subscription, reason: String) {
        runCatching { subscription.callback.onClosed(reason) }
    }

    private fun awaitEndpoint(): File {
        repeat(ENDPOINT_RETRIES) {
            val endpoint = DaemonApiRuntime.endpointFile(filesDir)
            if (endpoint != null && endpoint.exists()) return endpoint
            Thread.sleep(ENDPOINT_RETRY_DELAY_MS)
        }
        val state = DaemonStateStore.state.value
        throw IllegalStateException(
            state.lastError ?: "VeilKnit daemon API is not ready (${state.status})",
        )
    }

    private class Subscription(
        val id: Long,
        val callback: IVeilKnitStreamCallback,
    ) {
        @Volatile var closed: Boolean = false
        @Volatile var socket: LocalSocket? = null
        @Volatile var job: Job? = null

        fun close() {
            closed = true
            runCatching { socket?.close() }
            job?.cancel()
        }
    }

    companion object {
        val PERMISSION: String = BuildConfig.APPLICATION_ID + ".permission.BIND_VEILKNIT_API"
        private const val MAX_REQUEST_BYTES = 1024 * 1024
        private const val RESPONSE_TIMEOUT_MS = 90_000
        private const val ENDPOINT_RETRIES = 80
        private const val ENDPOINT_RETRY_DELAY_MS = 100L
    }
}
