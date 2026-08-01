package com.example.veilknit_mailer

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.os.IBinder
import android.util.Base64
import com.example.veilknit_deamon.ipc.IVeilKnitApi
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject
import java.io.ByteArrayOutputStream
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.security.SecureRandom
import java.util.concurrent.atomic.AtomicLong
import javax.crypto.Mac
import javax.crypto.spec.SecretKeySpec

data class KnownNode(
    val mainDht: String,
    val verified: Boolean,
    val verificationState: String,
    val presenceState: String,
    val lastSeen: Long,
    val lastOnline: Long,
    val mailboxCapable: Boolean,
)

data class InboxSummary(
    val messageId: String,
    val senderMainDht: String,
    val postedAt: Long,
    val receivedAt: Long,
    val plaintextLength: Int,
    val read: Boolean,
)

data class InboxMessage(
    val messageId: String,
    val senderMainDht: String,
    val recipientMainDht: String,
    val postedAt: Long,
    val receivedAt: Long,
    val expiresAt: Long,
    val text: String,
)

private data class SavedCredential(
    val secretHex: String,
    val generation: Long,
    val username: String = "",
    val mainDht: String = "",
)

class AuthorizationPending(val requestId: Long) : Exception(
    "Approve VeilKnit Mailer request #$requestId in the daemon",
)

class MailerApiClient(private val context: Context) {
    private val requestIds = AtomicLong(1)
    private val preferences = context.getSharedPreferences("veilknit_mailer_api", Context.MODE_PRIVATE)
    private var api: IVeilKnitApi? = null
    private var connection: ServiceConnection? = null
    private var sessionToken: String? = null

    suspend fun connectAndAuthenticate() = withContext(Dispatchers.IO) {
        connect()
        if (trySavedCredentials()) return@withContext
        ensureRegistration()
    }

    suspend fun freshConnect() = withContext(Dispatchers.IO) {
        connect()
        sessionToken = null
        preferences.edit()
            .remove(KEY_REGISTRATION_ID)
            .remove(KEY_REGISTRATION_TOKEN)
            .apply()
        if (trySavedCredentials()) return@withContext
        requestRegistration()
    }

    fun close() {
        connection?.let { runCatching { context.unbindService(it) } }
        connection = null
        api = null
        sessionToken = null
    }

    suspend fun checkAuthorizationAndAuthenticate() = withContext(Dispatchers.IO) {
        connect()
        val requestId = preferences.getLong(KEY_REGISTRATION_ID, 0L)
        val token = preferences.getString(KEY_REGISTRATION_TOKEN, null)
            ?: throw IllegalStateException("No pending authorization request")
        if (requestId == 0L) throw IllegalStateException("No pending authorization request")
        val result = transact(
            JSONObject()
                .put("action", "get_app_registration_status")
                .put("registration_request_id", requestId)
                .put("request_token_hex", token),
            authenticated = false,
        )
        when (result.getString("type")) {
            "app_registration_approved" -> {
                val credential = SavedCredential(
                    secretHex = result.getString("secret_hex"),
                    generation = result.getLong("credential_generation"),
                )
                preferences.edit().remove(KEY_REGISTRATION_ID).remove(KEY_REGISTRATION_TOKEN).apply()
                saveCredential(authenticate(credential))
            }
            "app_registration_still_pending" -> throw AuthorizationPending(requestId)
            "app_registration_rejected" -> throw IllegalStateException(
                result.optString("reason", "Authorization was rejected"),
            )
            "app_registration_expired" -> {
                preferences.edit().remove(KEY_REGISTRATION_ID).remove(KEY_REGISTRATION_TOKEN).apply()
                requestRegistration()
            }
            else -> throw IllegalStateException("Unexpected registration response: ${result.getString("type")}")
        }
    }

    suspend fun listKnownNodes(): List<KnownNode> = withContext(Dispatchers.IO) {
        val result = authenticated("list_known_nodes")
        val nodes = result.getJSONArray("nodes")
        buildList(nodes.length()) {
            for (index in 0 until nodes.length()) {
                val node = nodes.getJSONObject(index)
                add(
                    KnownNode(
                        mainDht = node.getString("main_dht"),
                        verified = node.getBoolean("verified"),
                        verificationState = node.getString("verification_state"),
                        presenceState = node.getString("presence_state"),
                        lastSeen = node.optLong("last_seen", 0),
                        lastOnline = node.optLong("last_online", 0),
                        mailboxCapable = node.optBoolean("mailbox_capable", false),
                    ),
                )
            }
        }
    }

    suspend fun listInbox(): List<InboxSummary> = withContext(Dispatchers.IO) {
        val result = authenticated("list_inbox")
        val messages = result.getJSONArray("messages")
        buildList(messages.length()) {
            for (index in 0 until messages.length()) {
                val message = messages.getJSONObject(index)
                add(
                    InboxSummary(
                        messageId = message.getString("message_id_hex"),
                        senderMainDht = message.getString("sender_main_dht"),
                        postedAt = message.getLong("posted_at"),
                        receivedAt = message.getLong("received_at"),
                        plaintextLength = message.getInt("plaintext_len"),
                        read = message.getBoolean("read"),
                    ),
                )
            }
        }
    }

    suspend fun readInbox(messageId: String): InboxMessage = withContext(Dispatchers.IO) {
        val result = authenticated("read_inbox") { put("message_id_hex", messageId) }
        val message = result.getJSONObject("message")
        val bytes = Base64.decode(message.getString("payload_base64"), Base64.DEFAULT)
        InboxMessage(
            messageId = message.getString("message_id_hex"),
            senderMainDht = message.getString("sender_main_dht"),
            recipientMainDht = message.getString("recipient_main_dht"),
            postedAt = message.getLong("posted_at"),
            receivedAt = message.getLong("received_at"),
            expiresAt = message.getLong("expires_at"),
            text = bytes.toString(Charsets.UTF_8),
        )
    }

    suspend fun deleteInbox(messageId: String) = withContext(Dispatchers.IO) {
        authenticated("delete_inbox") { put("message_id_hex", messageId) }
        Unit
    }

    suspend fun triggerRetrieval() = withContext(Dispatchers.IO) {
        authenticated("trigger_message_retrieval")
        Unit
    }

    suspend fun sendMessage(recipient: String, text: String): String = withContext(Dispatchers.IO) {
        require(text.toByteArray(Charsets.UTF_8).size <= 8 * 1024) { "Messages are limited to 8 KiB" }
        val result = authenticated("send_message") {
            put("recipient_main_dht", recipient)
            put("payload_base64", Base64.encodeToString(text.toByteArray(Charsets.UTF_8), Base64.NO_WRAP))
            put("await_response", false)
        }
        result.getString("message_id_hex")
    }

    private suspend fun connect(): IVeilKnitApi {
        api?.let { return it }
        val deferred = CompletableDeferred<IVeilKnitApi>()
        val serviceConnection = object : ServiceConnection {
            override fun onServiceConnected(name: ComponentName?, binder: IBinder?) {
                deferred.complete(IVeilKnitApi.Stub.asInterface(binder))
            }
            override fun onServiceDisconnected(name: ComponentName?) {
                api = null
                sessionToken = null
            }
            override fun onNullBinding(name: ComponentName?) {
                deferred.completeExceptionally(IllegalStateException("VeilKnit daemon returned a null API binding"))
            }
        }
        connection = serviceConnection
        val intent = Intent(DAEMON_ACTION).setPackage(DAEMON_PACKAGE)
        if (!context.bindService(intent, serviceConnection, Context.BIND_AUTO_CREATE)) {
            throw IllegalStateException("VeilKnit Daemon is not installed or its API service is unavailable")
        }
        return deferred.await().also { api = it }
    }

    private fun ensureRegistration() {
        val pendingId = preferences.getLong(KEY_REGISTRATION_ID, 0L)
        if (pendingId != 0L) throw AuthorizationPending(pendingId)
        requestRegistration()
    }

    private fun requestRegistration(): Nothing {
        val token = ByteArray(32).also(SecureRandom()::nextBytes).toHex()
        val result = transact(
            JSONObject()
                .put("action", "request_app_registration")
                .put("app_id", APP_ID)
                .put("display_name", "VeilKnit Mailer")
                .put("requested_capabilities", JSONArray(CAPABILITIES))
                .put("request_token_hex", token),
            authenticated = false,
        )
        val requestId = result.getLong("request_id")
        preferences.edit()
            .putLong(KEY_REGISTRATION_ID, requestId)
            .putString(KEY_REGISTRATION_TOKEN, token)
            .apply()
        throw AuthorizationPending(requestId)
    }

    private fun authenticate(credential: SavedCredential): SavedCredential {
        val challenge = transact(
            JSONObject()
                .put("action", "begin_authentication")
                .put("app_id", APP_ID)
                .put("requested_capabilities", JSONArray(CAPABILITIES)),
            authenticated = false,
        )
        val proof = computeProof(credential.secretHex.hexToBytes(), challenge)
        val result = transact(
            JSONObject()
                .put("action", "finish_authentication")
                .put("app_id", APP_ID)
                .put("challenge_id", challenge.getLong("challenge_id"))
                .put("proof_hex", proof.toHex()),
            authenticated = false,
        )
        sessionToken = result.getString("session_token_hex")
        val identity = authenticated("get_identity")
        return credential.copy(
            username = identity.optString("username"),
            mainDht = identity.optString("main_dht"),
        )
    }

    private fun trySavedCredentials(): Boolean {
        for (credential in loadCredentials()) {
            try {
                sessionToken = null
                saveCredential(authenticate(credential))
                return true
            } catch (_: Throwable) {
                sessionToken = null
            }
        }
        return false
    }

    private fun loadCredentials(): List<SavedCredential> {
        val credentials = mutableListOf<SavedCredential>()
        val stored = preferences.getString(KEY_CREDENTIALS, null)
        if (!stored.isNullOrBlank()) {
            runCatching {
                val array = JSONArray(stored)
                for (index in 0 until array.length()) {
                    val value = array.getJSONObject(index)
                    val secret = value.optString("secret_hex")
                    if (secret.isNotBlank()) {
                        credentials += SavedCredential(
                            secretHex = secret,
                            generation = value.optLong("credential_generation", 0L),
                            username = value.optString("username"),
                            mainDht = value.optString("main_dht"),
                        )
                    }
                }
            }
        }

        // Migrate the original single-account credential without losing it.
        preferences.getString(KEY_SECRET, null)?.takeIf { it.isNotBlank() }?.let { secret ->
            credentials += SavedCredential(
                secretHex = secret,
                generation = preferences.getLong(KEY_GENERATION, 0L),
            )
        }
        val activeSecret = preferences.getString(KEY_SECRET, null)
        return credentials
            .distinctBy { it.secretHex }
            .sortedByDescending { it.secretHex == activeSecret }
    }

    private fun saveCredential(credential: SavedCredential) {
        val credentials = loadCredentials().toMutableList()
        val index = credentials.indexOfFirst { it.secretHex == credential.secretHex }
        if (index >= 0) credentials[index] = credential else credentials += credential

        val array = JSONArray()
        credentials.forEach { saved ->
            array.put(
                JSONObject()
                    .put("secret_hex", saved.secretHex)
                    .put("credential_generation", saved.generation)
                    .put("username", saved.username)
                    .put("main_dht", saved.mainDht),
            )
        }
        preferences.edit()
            .putString(KEY_CREDENTIALS, array.toString())
            .putString(KEY_SECRET, credential.secretHex)
            .putLong(KEY_GENERATION, credential.generation)
            .apply()
    }

    private fun authenticated(action: String, extra: JSONObject.() -> Unit = {}): JSONObject {
        val token = sessionToken ?: throw IllegalStateException("Mailer is not authenticated")
        val request = JSONObject().put("action", action).put("session_token", token)
        request.extra()
        return transact(request, authenticated = true)
    }

    private fun transact(request: JSONObject, authenticated: Boolean): JSONObject {
        val service = api ?: throw IllegalStateException("VeilKnit daemon API is not connected")
        val envelope = JSONObject()
            .put("protocol_version", 3)
            .put("request_id", requestIds.getAndIncrement())
        val keys = request.keys()
        while (keys.hasNext()) {
            val key = keys.next()
            envelope.put(key, request.get(key))
        }
        val response = JSONObject(service.transact(envelope.toString()))
        if (!response.optBoolean("ok", false)) {
            val error = response.optJSONObject("error")
            val code = error?.optString("code") ?: "daemon_error"
            val message = error?.optString("message") ?: "VeilKnit daemon rejected the request"
            if (authenticated && code in setOf("invalid_session", "session_expired")) sessionToken = null
            throw IllegalStateException("$code: $message")
        }
        return response.getJSONObject("result")
    }

    private fun computeProof(secret: ByteArray, challenge: JSONObject): ByteArray {
        val output = ByteArrayOutputStream()
        output.write("veilknit/app-auth/v2".toByteArray(Charsets.UTF_8))
        val appBytes = APP_ID.toByteArray(Charsets.UTF_8)
        output.write(leInt(appBytes.size))
        output.write(appBytes)
        output.write(leLong(challenge.getLong("challenge_id")))
        output.write(challenge.getString("nonce_hex").hexToBytes())
        output.write(leLong(challenge.getLong("issued_at")))
        output.write(leLong(challenge.getLong("expires_at")))
        output.write(leLong(challenge.getLong("credential_generation")))
        val capabilities = challenge.getJSONArray("requested_capabilities")
        output.write(leInt(capabilities.length()))
        for (index in 0 until capabilities.length()) {
            output.write(capabilities.getString(index).toByteArray(Charsets.UTF_8))
            output.write(0)
        }
        return Mac.getInstance("HmacSHA256").run {
            init(SecretKeySpec(secret, "HmacSHA256"))
            doFinal(output.toByteArray())
        }
    }

    private fun leInt(value: Int): ByteArray = ByteBuffer.allocate(4)
        .order(ByteOrder.LITTLE_ENDIAN).putInt(value).array()
    private fun leLong(value: Long): ByteArray = ByteBuffer.allocate(8)
        .order(ByteOrder.LITTLE_ENDIAN).putLong(value).array()

    companion object {
        const val APP_ID = "veilknit.mailer"
        private const val DAEMON_PACKAGE = "com.example.veilknit_deamon"
        private const val DAEMON_ACTION = "com.example.veilknit_deamon.BIND_LOCAL_API"
        private val CAPABILITIES = listOf(
            "SendMessages",
            "ReceiveMessages",
            "ReadPublicProfiles",
            "SubscribeNetworkStatus",
        )
        private const val KEY_SECRET = "secret_hex"
        private const val KEY_GENERATION = "credential_generation"
        private const val KEY_CREDENTIALS = "saved_credentials_json"
        private const val KEY_REGISTRATION_ID = "registration_id"
        private const val KEY_REGISTRATION_TOKEN = "registration_token"
    }
}

private fun ByteArray.toHex(): String = joinToString("") { "%02x".format(it) }
private fun String.hexToBytes(): ByteArray {
    require(length % 2 == 0) { "Invalid hexadecimal value" }
    return ByteArray(length / 2) { index -> substring(index * 2, index * 2 + 2).toInt(16).toByte() }
}
