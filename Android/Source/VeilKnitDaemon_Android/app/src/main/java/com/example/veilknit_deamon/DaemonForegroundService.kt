package com.example.veilknit_deamon

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.os.PowerManager
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import java.util.concurrent.atomic.AtomicBoolean

class DaemonForegroundService : Service() {
    private val serviceScope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val startedNative = AtomicBoolean(false)
    private var pollJob: Job? = null
    private var wakeLock: PowerManager.WakeLock? = null

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
        DaemonStateStore.markServiceRunning()
        acquireWakeLock()
        pollJob = serviceScope.launch { pollNativeBridge() }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_START -> {
                promoteToForeground("Starting…")
                val username = intent.getStringExtra(EXTRA_USERNAME).orEmpty()
                val password = intent.getStringExtra(EXTRA_PASSWORD).orEmpty()
                val signup = intent.getBooleanExtra(EXTRA_SIGNUP, false)
                DaemonApiRuntime.setUsername(username)
                startNativeDaemon(username, password, signup)
            }

            ACTION_COMMAND -> {
                val commands = intent.getStringArrayListExtra(EXTRA_COMMANDS).orEmpty()
                commands.forEach { command ->
                    if (!NativeDaemonBridge.sendCommand(command)) {
                        DaemonStateStore.setError("The native daemon is not accepting commands.")
                    }
                }
            }

            ACTION_STOP -> requestGracefulStop()
            else -> promoteToForeground(DaemonStateStore.state.value.status)
        }
        return START_NOT_STICKY
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onTaskRemoved(rootIntent: Intent?) {
        // The daemon remains user-visible through its foreground notification.
        super.onTaskRemoved(rootIntent)
    }

    override fun onDestroy() {
        if (NativeDaemonBridge.isRunning()) {
            NativeDaemonBridge.requestStop()
        }
        pollJob?.cancel()
        wakeLock?.let { lock -> if (lock.isHeld) lock.release() }
        serviceScope.cancel()
        DaemonApiRuntime.clearUsername()
        DaemonStateStore.markStopped()
        super.onDestroy()
    }

    private fun startNativeDaemon(username: String, password: String, signup: Boolean) {
        if (!startedNative.compareAndSet(false, true)) return

        if (username.isBlank() || password.isEmpty()) {
            DaemonStateStore.setError("Username and password are required.")
            updateNotification("Could not start: missing credentials")
            stopSelf()
            return
        }

        serviceScope.launch {
            DaemonStateStore.setStatus(if (signup) "Creating account…" else "Logging in…")
            updateNotification(DaemonStateStore.state.value.status)

	    val started = NativeDaemonBridge.start(
	        context = applicationContext,
	        dataDirectory = filesDir.absolutePath,
	        signup = signup,
	        username = username,
	        password = password,
	    )
            if (!started) {
                val detail = NativeDaemonBridge.loadError
                    ?: "The Rust daemon refused to start. Check the native build output."
                DaemonStateStore.setError(detail)
                updateNotification("Native daemon failed to start")
                delay(2_000)
                stopForeground(STOP_FOREGROUND_REMOVE)
                stopSelf()
            }
        }
    }

    private suspend fun pollNativeBridge() {
        var lastNotificationText = ""
        var observedRunning = false
        var stoppedAfterStartTicks = 0

        while (serviceScope.isActive) {
            val lines = NativeDaemonBridge.drainLogs()
            if (lines.isNotEmpty()) {
                DaemonStateStore.appendLogs(lines)
            }

            val running = NativeDaemonBridge.isRunning()
            if (running) {
                observedRunning = true
                stoppedAfterStartTicks = 0
            } else if (startedNative.get()) {
                stoppedAfterStartTicks += 1
            }
            DaemonStateStore.markNativeRunning(running)

            val state = DaemonStateStore.state.value
            val notificationText = when {
                state.ready && state.mainDhtKey.isNotBlank() ->
                    "Running • ${state.mainDhtKey.take(18)}…"
                else -> state.status
            }
            if (notificationText != lastNotificationText && state.serviceRunning) {
                updateNotification(notificationText)
                lastNotificationText = notificationText
            }

            if ((observedRunning || stoppedAfterStartTicks >= 4) && !running && state.serviceRunning) {
                stopForeground(STOP_FOREGROUND_REMOVE)
                stopSelf()
                break
            }
            delay(250)
        }
    }

    private fun requestGracefulStop() {
        DaemonStateStore.setStatus("Stopping safely…")
        updateNotification("Saving state and stopping…")
        NativeDaemonBridge.requestStop()

        serviceScope.launch {
            while (NativeDaemonBridge.isRunning()) {
                delay(250)
            }
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
        }
    }

    private fun promoteToForeground(text: String) {
        val type = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE
        } else {
            0
        }
        ServiceCompat.startForeground(
            this,
            NOTIFICATION_ID,
            buildNotification(text),
            type,
        )
    }

    private fun updateNotification(text: String) {
        val manager = getSystemService(NotificationManager::class.java)
        manager.notify(NOTIFICATION_ID, buildNotification(text))
    }

    private fun buildNotification(text: String): Notification {
        val openIntent = Intent(this, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP
        }
        val openPendingIntent = PendingIntent.getActivity(
            this,
            1,
            openIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val stopIntent = Intent(this, DaemonForegroundService::class.java).apply {
            action = ACTION_STOP
        }
        val stopPendingIntent = PendingIntent.getService(
            this,
            2,
            stopIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )

        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(getString(R.string.app_name))
            .setContentText(text)
            .setContentIntent(openPendingIntent)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setCategory(NotificationCompat.CATEGORY_SERVICE)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .setVisibility(NotificationCompat.VISIBILITY_PRIVATE)
            .addAction(R.drawable.ic_notification, "Open", openPendingIntent)
            .addAction(android.R.drawable.ic_menu_close_clear_cancel, "Stop", stopPendingIntent)
            .build()
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val channel = NotificationChannel(
            CHANNEL_ID,
            getString(R.string.app_name),
            NotificationManager.IMPORTANCE_LOW,
        ).apply {
            description = getString(R.string.notification_channel_description)
            setShowBadge(false)
        }
        getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
    }

    private fun acquireWakeLock() {
        val powerManager = getSystemService(PowerManager::class.java)
        wakeLock = powerManager.newWakeLock(
            PowerManager.PARTIAL_WAKE_LOCK,
            "$packageName:veilknit-daemon",
        ).apply {
            setReferenceCounted(false)
            acquire()
        }
    }

    companion object {
        private const val CHANNEL_ID = "veilknit_daemon"
        private const val NOTIFICATION_ID = 2207
        private const val ACTION_START = "com.example.veilknit_deamon.START"
        private const val ACTION_COMMAND = "com.example.veilknit_deamon.COMMAND"
        private const val ACTION_STOP = "com.example.veilknit_deamon.STOP"
        private const val EXTRA_USERNAME = "username"
        private const val EXTRA_PASSWORD = "password"
        private const val EXTRA_SIGNUP = "signup"
        private const val EXTRA_COMMANDS = "commands"

        fun start(context: Context, username: String, password: String, signup: Boolean) {
            val intent = Intent(context, DaemonForegroundService::class.java).apply {
                action = ACTION_START
                putExtra(EXTRA_USERNAME, username)
                putExtra(EXTRA_PASSWORD, password)
                putExtra(EXTRA_SIGNUP, signup)
            }
            ContextCompat.startForegroundService(context, intent)
        }

        fun sendCommands(context: Context, commands: List<String>) {
            if (commands.isEmpty()) return
            val intent = Intent(context, DaemonForegroundService::class.java).apply {
                action = ACTION_COMMAND
                putStringArrayListExtra(EXTRA_COMMANDS, ArrayList(commands))
            }
            context.startService(intent)
        }

        fun stop(context: Context) {
            val intent = Intent(context, DaemonForegroundService::class.java).apply {
                action = ACTION_STOP
            }
            context.startService(intent)
        }
    }
}
