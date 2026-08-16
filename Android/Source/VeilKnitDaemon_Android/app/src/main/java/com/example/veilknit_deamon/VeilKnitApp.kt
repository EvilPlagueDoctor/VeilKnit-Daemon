package com.example.veilknit_deamon

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Environment
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.ExpandLess
import androidx.compose.material.icons.filled.ExpandMore
import androidx.compose.material.icons.filled.PowerSettingsNew
import androidx.compose.material.icons.filled.Save
import androidx.compose.material.icons.filled.Visibility
import androidx.compose.material.icons.filled.VisibilityOff
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.ScrollableTabRow
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.foundation.clickable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.example.veilknit_deamon.ui.theme.VeilBorder
import com.example.veilknit_deamon.ui.theme.VeilEdit
import com.example.veilknit_deamon.ui.theme.VeilMuted
import com.example.veilknit_deamon.ui.theme.VeilPanel
import com.example.veilknit_deamon.ui.theme.VeilRed
import com.example.veilknit_deamon.ui.theme.VeilSuccess
import com.example.veilknit_deamon.ui.theme.VeilText
import com.example.veilknit_deamon.ui.theme.VeilWarning
import com.example.veilknit_deamon.ui.theme.VeilWindow
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File

private enum class AppTab(val title: String, val category: LogCategory) {
    Applications("Applications", LogCategory.Applications),
    Backup("Backup", LogCategory.Overview),
    Overview("Overview", LogCategory.Overview),
    Handshake("Handshake", LogCategory.Handshake),
    Network("Network", LogCategory.Network),
    Headers("Headers", LogCategory.Headers),
    Dht("DHT", LogCategory.Dht),
    Mailbox("Mailbox", LogCategory.Mailbox),
    Logs("All logs", LogCategory.All),
}

@Composable
fun VeilKnitApp() {
    val state by DaemonStateStore.state.collectAsStateWithLifecycle()
    val snackbarHostState = remember { SnackbarHostState() }
    val context = LocalContext.current
    var language by remember { mutableStateOf(LanguagePreferences.load(context)) }
    UiStrings.current = language

    CompositionLocalProvider(LocalUiLanguage provides language) {
        Scaffold(
            containerColor = VeilWindow,
            snackbarHost = { SnackbarHost(snackbarHostState) },
        ) { innerPadding ->
            if (!state.serviceRunning && !state.nativeRunning) {
                LoginScreen(
                    modifier = Modifier.padding(innerPadding),
                    snackbarHostState = snackbarHostState,
                    lastError = state.lastError,
                    language = language,
                    onLanguageChange = {
                        language = it
                        UiStrings.current = it
                        LanguagePreferences.save(context, it)
                    },
                )
            } else {
                DaemonScreen(
                    modifier = Modifier.padding(innerPadding),
                    state = state,
                    snackbarHostState = snackbarHostState,
                )
            }
        }
    }
}

@Composable
private fun LoginScreen(
    modifier: Modifier,
    snackbarHostState: SnackbarHostState,
    lastError: String?,
    language: UiLanguage,
    onLanguageChange: (UiLanguage) -> Unit,
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var username by rememberSaveable { mutableStateOf("") }
    var password by rememberSaveable { mutableStateOf("") }
    var passwordVisible by rememberSaveable { mutableStateOf(false) }
    val restoreLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.OpenDocument(),
    ) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        val backupPassphrase = password
        if (backupPassphrase.isEmpty()) {
            scope.launch {
                snackbarHostState.showSnackbar(
                    tr("Enter the backup passphrase in the Password field first."),
                )
            }
            return@rememberLauncherForActivityResult
        }
        scope.launch {
            val message = withContext(Dispatchers.IO) {
                val temporary = File.createTempFile("veilknit-restore-", ".veilknit-backup", context.cacheDir)
                try {
                    context.contentResolver.openInputStream(uri)?.use { input ->
                        temporary.outputStream().use { output -> input.copyTo(output) }
                    } ?: return@withContext tr("Could not open the selected backup file.")
                    NativeDaemonBridge.restoreBackup(
                        dataDirectory = context.filesDir.absolutePath,
                        backupPath = temporary.absolutePath,
                        passphrase = backupPassphrase,
                    )
                } finally {
                    temporary.delete()
                }
            }
            password = ""
            snackbarHostState.showSnackbar(message)
        }
    }

    fun start(signup: Boolean) {
        val trimmedUsername = username.trim()
        when {
            !Regex("^[A-Za-z0-9_-]+$").matches(trimmedUsername) -> scope.launch {
                snackbarHostState.showSnackbar(
                    tr("Usernames may contain letters, numbers, underscores, and hyphens."),
                )
            }
            password.isEmpty() || '\n' in password || '\r' in password -> scope.launch {
                snackbarHostState.showSnackbar(tr("Enter a password without line breaks."))
            }
            else -> {
                DaemonStateStore.markServiceRunning(
                    if (signup) tr("Creating account…") else tr("Logging in…"),
                )
                DaemonForegroundService.start(
                    context = context,
                    username = trimmedUsername,
                    password = password,
                    signup = signup,
                )
                password = ""
            }
        }
    }

    Column(
        modifier = modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 24.dp, vertical = 28.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Card(
            modifier = Modifier.size(160.dp),
            colors = CardDefaults.cardColors(containerColor = Color.White),
            shape = RoundedCornerShape(28.dp),
        ) {
            Image(
                painter = painterResource(R.drawable.veilknit_logo),
                contentDescription = "VeilKnit logo",
                modifier = Modifier
                    .fillMaxSize()
                    .padding(10.dp),
                contentScale = ContentScale.Fit,
            )
        }
        Spacer(Modifier.height(22.dp))
        Text(
            text = stringResource(R.string.app_name),
            style = MaterialTheme.typography.headlineMedium,
            fontWeight = FontWeight.Bold,
            color = VeilText,
        )
        Text(
            text = tr("Android foreground node") + " • ${stringResource(R.string.instance_name)}",
            style = MaterialTheme.typography.bodyMedium,
            color = VeilMuted,
        )
        Spacer(Modifier.height(10.dp))
        LanguageSelector(language = language, onLanguageChange = onLanguageChange)
        Spacer(Modifier.height(20.dp))

        if (!NativeDaemonBridge.isLibraryLoaded) {
            WarningCard(
                tr("Native Rust library is not present. Build with cargo-ndk before installing.") + " " +
                    (NativeDaemonBridge.loadError ?: ""),
            )
            Spacer(Modifier.height(16.dp))
        }
        if (!lastError.isNullOrBlank()) {
            WarningCard(lastError)
            Spacer(Modifier.height(16.dp))
        }

        OutlinedTextField(
            value = username,
            onValueChange = { username = it },
            label = { Text(tr("Username")) },
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(12.dp))
        OutlinedTextField(
            value = password,
            onValueChange = { password = it },
            label = { Text(tr("Password")) },
            singleLine = true,
            visualTransformation = if (passwordVisible) {
                VisualTransformation.None
            } else {
                PasswordVisualTransformation()
            },
            trailingIcon = {
                IconButton(onClick = { passwordVisible = !passwordVisible }) {
                    Icon(
                        imageVector = if (passwordVisible) Icons.Default.VisibilityOff
                        else Icons.Default.Visibility,
                        contentDescription = if (passwordVisible) tr("Hide password") else tr("Show password"),
                    )
                }
            },
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(18.dp))
        Button(
            onClick = { start(signup = false) },
            enabled = NativeDaemonBridge.isLibraryLoaded,
            modifier = Modifier.fillMaxWidth(),
            colors = ButtonDefaults.buttonColors(containerColor = VeilRed),
        ) {
            Text(tr("Log in"))
        }
        Spacer(Modifier.height(8.dp))
        TextButton(
            onClick = { start(signup = true) },
            enabled = NativeDaemonBridge.isLibraryLoaded,
            modifier = Modifier.fillMaxWidth(),
        ) {
            Text(tr("Create account"), color = VeilRed)
        }
        TextButton(
            onClick = { restoreLauncher.launch(arrayOf("application/octet-stream", "application/zip", "*/*")) },
            enabled = NativeDaemonBridge.isLibraryLoaded,
            modifier = Modifier.fillMaxWidth(),
        ) {
            Text(tr("Restore encrypted backup"), color = VeilRed)
        }
        Spacer(Modifier.height(18.dp))
        Text(
            text = tr("The password is passed directly to the in-process Rust daemon and is not stored by the Android UI."),
            style = MaterialTheme.typography.bodySmall,
            color = VeilMuted,
        )
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun DaemonScreen(
    modifier: Modifier,
    state: DaemonUiState,
    snackbarHostState: SnackbarHostState,
) {
    var selectedTab by rememberSaveable { mutableIntStateOf(0) }
    var showHelp by rememberSaveable { mutableStateOf(false) }
    val context = LocalContext.current

    LaunchedEffect(state.ready) {
        if (state.ready) {
            send(context, "walk-settings", "headers", "summary", "app-pending")
            while (true) {
                delay(15_000)
                send(context, "summary", "app-pending")
            }
        }
    }

    Column(modifier.fillMaxSize()) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .background(VeilPanel)
                .padding(horizontal = 14.dp, vertical = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Card(
                modifier = Modifier.size(48.dp),
                colors = CardDefaults.cardColors(containerColor = Color.White),
                shape = RoundedCornerShape(12.dp),
            ) {
                Image(
                    painter = painterResource(R.drawable.veilknit_logo),
                    contentDescription = null,
                    modifier = Modifier
                        .fillMaxSize()
                        .padding(3.dp),
                    contentScale = ContentScale.Fit,
                )
            }
            Spacer(Modifier.width(12.dp))
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    stringResource(R.string.app_name),
                    fontWeight = FontWeight.Bold,
                    color = VeilText,
                    maxLines = 1,
                )
                Row(verticalAlignment = Alignment.CenterVertically) {
                    StatusDot(state)
                    Spacer(Modifier.width(7.dp))
                    Text(
                        state.status,
                        style = MaterialTheme.typography.bodySmall,
                        color = VeilMuted,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
            TextButton(onClick = { showHelp = true }) { Text(tr("Help"), color = VeilText) }
            IconButton(onClick = { DaemonForegroundService.stop(context) }) {
                Icon(
                    Icons.Default.PowerSettingsNew,
                    contentDescription = "Stop daemon safely",
                    tint = VeilRed,
                )
            }
        }

        ScrollableTabRow(
            selectedTabIndex = selectedTab,
            containerColor = VeilPanel,
            contentColor = VeilRed,
            edgePadding = 8.dp,
            divider = { HorizontalDivider(color = VeilBorder) },
        ) {
            AppTab.entries.forEachIndexed { index, tab ->
                Tab(
                    selected = selectedTab == index,
                    onClick = { selectedTab = index },
                    text = { Text(tr(tab.title)) },
                )
            }
        }

        Box(modifier = Modifier.fillMaxSize()) {
            when (AppTab.entries[selectedTab]) {
                AppTab.Overview -> OverviewPage(state, snackbarHostState)
                AppTab.Handshake -> HandshakePage(state, snackbarHostState)
                AppTab.Network -> NetworkPage(state, snackbarHostState)
                AppTab.Headers -> HeadersPage(state, snackbarHostState)
                AppTab.Dht -> DhtPage(state, snackbarHostState)
                AppTab.Mailbox -> MailboxPage(state, snackbarHostState)
                AppTab.Applications -> ApplicationsPage(state, snackbarHostState)
                AppTab.Backup -> BackupPage(state, snackbarHostState)
                AppTab.Logs -> LogsPage(state.logs)
            }
        }
    }


    if (showHelp) {
        AlertDialog(
            onDismissRequest = { showHelp = false },
            confirmButton = { TextButton(onClick = { showHelp = false }) { Text(tr("Got it")) } },
            dismissButton = {
                TextButton(
                    onClick = {
                        context.startActivity(
                            Intent(Intent.ACTION_VIEW, Uri.parse("https://discord.gg/yy5SMTuZY")),
                        )
                    },
                ) { Text("Discord") }
            },
            title = { Text("${stringResource(R.string.app_name)} • " + tr("Help")) },
            text = {
                Column {
                    Text(tr("APP_LINK_HELP"))
                    Spacer(Modifier.height(12.dp))
                    Text(
                        "Community/support: https://discord.gg/yy5SMTuZY\n" +
                            "Discord is an external service and is not required for VeilKnit.",
                        color = VeilMuted,
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
            },
        )
    }
}

@Composable
private fun StatusDot(state: DaemonUiState) {
    val color = when {
        state.ready -> VeilSuccess
        state.lastError != null -> VeilRed
        state.nativeRunning || state.serviceRunning -> VeilWarning
        else -> VeilMuted
    }
    Box(
        Modifier
            .size(8.dp)
            .clip(RoundedCornerShape(50))
            .background(color),
    )
}

@Composable
private fun OverviewPage(state: DaemonUiState, snackbarHostState: SnackbarHostState) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val key = state.mainDhtKey

    PageColumn {
        SectionTitle("Daemon")
        InfoCard {
            Row(verticalAlignment = Alignment.CenterVertically) {
                if (!state.ready && state.nativeRunning) {
                    CircularProgressIndicator(
                        modifier = Modifier.size(20.dp),
                        strokeWidth = 2.dp,
                        color = VeilRed,
                    )
                    Spacer(Modifier.width(10.dp))
                }
                Column(Modifier.weight(1f)) {
                    Text(localizedStatus(state.status), fontWeight = FontWeight.SemiBold)
                    Text(
                        if (state.ready) tr("Network services are ready.") else tr("The foreground service is active."),
                        style = MaterialTheme.typography.bodySmall,
                        color = VeilMuted,
                    )
                }
            }
        }

        Spacer(Modifier.height(16.dp))
        SectionTitle("Main DHT key")
        InfoCard {
            SelectionContainer {
                Text(
                    text = key.ifBlank { tr("The main key will appear after DHT setup.") },
                    modifier = Modifier.fillMaxWidth(),
                    fontFamily = FontFamily.Monospace,
                    color = if (key.isBlank()) VeilMuted else VeilText,
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
            Spacer(Modifier.height(12.dp))
            Button(
                onClick = {
                    copyText(context, tr("Main DHT key"), key)
                    scope.launch { snackbarHostState.showSnackbar(tr("Main DHT key copied.")) }
                },
                enabled = key.isNotBlank(),
                modifier = Modifier.fillMaxWidth(),
                colors = ButtonDefaults.buttonColors(containerColor = VeilRed),
            ) {
                Icon(Icons.Default.ContentCopy, contentDescription = null)
                Spacer(Modifier.width(8.dp))
                Text(tr("Copy key"))
            }
        }

        Spacer(Modifier.height(16.dp))
        TwoActionRow(
            leftText = "Save log",
            leftIcon = { Icon(Icons.Default.Save, contentDescription = null) },
            onLeft = {
                // Keep the daemon's full on-device session-log save, but also
                // place a practical diagnostic excerpt on the clipboard so it
                // can be pasted directly into a bug report.
                send(context, "U", "")
                val excerpt = state.logs.takeLast(1_000).joinToString("\n")
                if (excerpt.isNotBlank()) {
                    copyText(context, "VeilKnit daemon log (last 1000 lines)", excerpt)
                    scope.launch {
                        snackbarHostState.showSnackbar(
                            "Log saved; last ${state.logs.size.coerceAtMost(1_000)} lines copied."
                        )
                    }
                } else {
                    scope.launch { snackbarHostState.showSnackbar(tr("No log lines to copy yet.")) }
                }
            },
            rightText = "Stop safely",
            rightIcon = { Icon(Icons.Default.PowerSettingsNew, contentDescription = null) },
            onRight = { DaemonForegroundService.stop(context) },
            enabled = state.nativeRunning,
        )

        Spacer(Modifier.height(18.dp))
        SectionTitle("Recent overview log")
        LogPanel(state.logs.forCategory(LogCategory.Overview).takeLast(250))
    }
}

@Composable
private fun HandshakePage(state: DaemonUiState, snackbarHostState: SnackbarHostState) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var peerKey by rememberSaveable { mutableStateOf("") }

    PageColumn {
        SectionTitle("Peer handshake")
        SingleLineField(peerKey, { peerKey = it }, "Peer VLD0 key")
        Spacer(Modifier.height(10.dp))
        TwoActionRow(
            leftText = "Establish",
            onLeft = {
                if (!looksLikeRecordKey(peerKey)) {
                    scope.launch { snackbarHostState.showSnackbar(tr("Paste a VLD0: DHT record key first.")) }
                } else send(context, "H", peerKey.trim())
            },
            rightText = "Check status",
            onRight = {
                if (!looksLikeRecordKey(peerKey)) {
                    scope.launch { snackbarHostState.showSnackbar(tr("Paste a VLD0: DHT record key first.")) }
                } else send(context, "K", peerKey.trim())
            },
            enabled = state.ready,
        )
        Spacer(Modifier.height(18.dp))
        SectionTitle("Handshake log")
        LogPanel(state.logs.forCategory(LogCategory.Handshake).takeLast(500))
    }
}

@Composable
private fun NetworkPage(state: DaemonUiState, snackbarHostState: SnackbarHostState) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()

    var normalMinHops by rememberSaveable { mutableStateOf(state.walkSettings.normalMinHops) }
    var normalMaxHops by rememberSaveable { mutableStateOf(state.walkSettings.normalMaxHops) }
    var normalMinSeconds by rememberSaveable { mutableStateOf(state.walkSettings.normalMinSeconds) }
    var normalTargetSeconds by rememberSaveable { mutableStateOf(state.walkSettings.normalTargetSeconds) }
    var normalMaxSeconds by rememberSaveable { mutableStateOf(state.walkSettings.normalMaxSeconds) }
    var mailMinHops by rememberSaveable { mutableStateOf(state.walkSettings.mailMinHops) }
    var mailMaxHops by rememberSaveable { mutableStateOf(state.walkSettings.mailMaxHops) }
    var mailMinSeconds by rememberSaveable { mutableStateOf(state.walkSettings.mailMinSeconds) }
    var mailTargetSeconds by rememberSaveable { mutableStateOf(state.walkSettings.mailTargetSeconds) }
    var mailMaxSeconds by rememberSaveable { mutableStateOf(state.walkSettings.mailMaxSeconds) }
    var automaticMailMode by rememberSaveable { mutableStateOf(state.walkSettings.automaticMailMode) }

    LaunchedEffect(state.walkSettings) {
        normalMinHops = state.walkSettings.normalMinHops
        normalMaxHops = state.walkSettings.normalMaxHops
        normalMinSeconds = state.walkSettings.normalMinSeconds
        normalTargetSeconds = state.walkSettings.normalTargetSeconds
        normalMaxSeconds = state.walkSettings.normalMaxSeconds
        mailMinHops = state.walkSettings.mailMinHops
        mailMaxHops = state.walkSettings.mailMaxHops
        mailMinSeconds = state.walkSettings.mailMinSeconds
        mailTargetSeconds = state.walkSettings.mailTargetSeconds
        mailMaxSeconds = state.walkSettings.mailMaxSeconds
        automaticMailMode = state.walkSettings.automaticMailMode
    }

    fun applySettings() {
        val raw = listOf(
            normalMinHops, normalMaxHops, normalMinSeconds, normalTargetSeconds, normalMaxSeconds,
            mailMinHops, mailMaxHops, mailMinSeconds, mailTargetSeconds, mailMaxSeconds,
        )
        val parsed = raw.map { it.toULongOrNull() }
        val valid = parsed.all { it != null } &&
            parsed[0]!! >= 1uL && parsed[0]!! <= parsed[1]!! &&
            parsed[2]!! <= parsed[3]!! && parsed[3]!! <= parsed[4]!! &&
            parsed[5]!! >= 1uL && parsed[5]!! <= parsed[6]!! &&
            parsed[7]!! <= parsed[8]!! && parsed[8]!! <= parsed[9]!!
        if (!valid) {
            scope.launch {
                snackbarHostState.showSnackbar(
                    "Use whole numbers; minimums must not exceed targets or maximums.",
                )
            }
            return
        }

        val command = buildString {
            append("walk-set ")
            append(raw.joinToString(" "))
            append(if (automaticMailMode) " 1" else " 0")
        }
        send(context, command)
    }

    PageColumn {
        NetworkSummaryDashboard(state.networkSummary)
        Spacer(Modifier.height(18.dp))
        SectionTitle("Normal walking mode")
        InfoCard {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Box(Modifier.weight(1f)) {
                    SingleLineField(normalMinHops, { normalMinHops = it }, "Minimum hops")
                }
                Box(Modifier.weight(1f)) {
                    SingleLineField(normalMaxHops, { normalMaxHops = it }, "Maximum hops")
                }
            }
            Spacer(Modifier.height(8.dp))
            SingleLineField(normalMinSeconds, { normalMinSeconds = it }, "Minimum interval (seconds)")
            Spacer(Modifier.height(8.dp))
            SingleLineField(normalTargetSeconds, { normalTargetSeconds = it }, "Target interval (seconds)")
            Spacer(Modifier.height(8.dp))
            SingleLineField(normalMaxSeconds, { normalMaxSeconds = it }, "Maximum interval (seconds)")
        }

        Spacer(Modifier.height(16.dp))
        SectionTitle("Mail walking mode")
        InfoCard {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Box(Modifier.weight(1f)) {
                    SingleLineField(mailMinHops, { mailMinHops = it }, "Minimum hops")
                }
                Box(Modifier.weight(1f)) {
                    SingleLineField(mailMaxHops, { mailMaxHops = it }, "Maximum hops")
                }
            }
            Spacer(Modifier.height(8.dp))
            SingleLineField(mailMinSeconds, { mailMinSeconds = it }, "Minimum interval (seconds)")
            Spacer(Modifier.height(8.dp))
            SingleLineField(mailTargetSeconds, { mailTargetSeconds = it }, "Target interval (seconds)")
            Spacer(Modifier.height(8.dp))
            SingleLineField(mailMaxSeconds, { mailMaxSeconds = it }, "Maximum interval (seconds)")
            Spacer(Modifier.height(8.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                Checkbox(
                    checked = automaticMailMode,
                    onCheckedChange = { automaticMailMode = it },
                    enabled = state.ready,
                )
                Text(tr("Use mail mode for automatic walks"), color = VeilText)
            }
        }

        Spacer(Modifier.height(12.dp))
        ActionButton("Apply and save walk settings", state.ready) { applySettings() }
        Spacer(Modifier.height(8.dp))
        TwoActionRow(
            leftText = "Start normal",
            onLeft = { send(context, "walk-normal") },
            rightText = "Start mail",
            onRight = { send(context, "walk-mail") },
            enabled = state.ready,
        )
        Spacer(Modifier.height(8.dp))
        TwoActionRow(
            leftText = "Walk status",
            onLeft = { send(context, "walk-settings", "P") },
            rightText = "Stop walk",
            onRight = { send(context, "O") },
            enabled = state.ready,
        )
        Spacer(Modifier.height(8.dp))
        TwoActionRow(
            leftText = "Route status",
            onLeft = { send(context, "C") },
            rightText = "Node list",
            onRight = { send(context, "I") },
            enabled = state.ready,
        )
        Spacer(Modifier.height(8.dp))
        ActionButton("Daemon status", state.ready) { send(context, "D") }
        Spacer(Modifier.height(6.dp))
        Text(
            "Applied settings are stored in your encrypted daemon account.",
            style = MaterialTheme.typography.bodySmall,
            color = VeilMuted,
        )

        Spacer(Modifier.height(18.dp))
        SectionTitle("Network log")
        LogPanel(state.logs.forCategory(LogCategory.Network).takeLast(700))
    }
}

@Composable
private fun NetworkSummaryDashboard(summary: NetworkSummaryUi) {
    SectionTitle("Network summary")
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        SummaryCard(
            title = "Topology",
            rows = listOf(
                "Verified" to summary.verified.toString(),
                "Candidates" to summary.candidates.toString(),
                "Authenticated" to summary.authenticated.toString(),
            ),
            modifier = Modifier.weight(1f),
        )
        SummaryCard(
            title = "Presence",
            rows = listOf(
                "Online" to summary.online.toString(),
                "Offline" to summary.offline.toString(),
                "Stale claim" to summary.stale.toString(),
                "Needs refresh" to summary.needsRefresh.toString(),
                "Unknown" to summary.unknown.toString(),
            ),
            modifier = Modifier.weight(1f),
        )
    }
    Spacer(Modifier.height(8.dp))
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        SummaryCard(
            title = "Header cache",
            rows = listOf(
                "Presence OK" to summary.presenceOk.toString(),
                "Read failed" to summary.presenceFailed.toString(),
                "Not checked" to summary.presenceUnread.toString(),
                "Active app info" to summary.appHeaders.toString(),
                "Mailbox capable" to summary.mailboxCapable.toString(),
            ),
            modifier = Modifier.weight(1f),
        )
        val progress = if (summary.walkTotal > 0) {
            "${summary.walkDone}/${summary.walkTotal}"
        } else {
            "—"
        }
        SummaryCard(
            title = "Activity",
            rows = listOf(
                "Walk" to summary.walkState.replaceFirstChar { it.uppercase() },
                "Progress" to progress,
                "New / updated" to "${summary.walkNew} / ${summary.walkUpdated}",
                "Reach / fail" to "${summary.walkReachable} / ${summary.walkUnreachable}",
                "App searches" to summary.appSearches.toString(),
                "Root lookups" to summary.rootLookups.toString(),
            ),
            modifier = Modifier.weight(1f),
        )
    }
}

@Composable
private fun SummaryCard(
    title: String,
    rows: List<Pair<String, String>>,
    modifier: Modifier = Modifier,
) {
    Card(
        modifier = modifier,
        colors = CardDefaults.cardColors(containerColor = VeilPanel),
        shape = RoundedCornerShape(12.dp),
    ) {
        Column(Modifier.padding(10.dp)) {
            Text(
                tr(title),
                color = VeilText,
                fontWeight = FontWeight.SemiBold,
                style = MaterialTheme.typography.bodyMedium,
            )
            Spacer(Modifier.height(5.dp))
            rows.forEach { (label, value) ->
                Row(Modifier.fillMaxWidth()) {
                    Text(
                        tr(label),
                        modifier = Modifier.weight(1f),
                        color = VeilMuted,
                        style = MaterialTheme.typography.bodySmall,
                    )
                    Text(
                        value,
                        color = VeilText,
                        fontFamily = FontFamily.Monospace,
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
            }
        }
    }
}

@Composable
private fun HeadersPage(state: DaemonUiState, snackbarHostState: SnackbarHostState) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()

    PageColumn {
        SectionTitle("Published main/presence header (subkey 0)")
        InfoCard {
            SelectionContainer {
                Text(
                    text = localizedStatus(state.mainHeader),
                    modifier = Modifier.fillMaxWidth(),
                    fontFamily = FontFamily.Monospace,
                    style = MaterialTheme.typography.bodySmall,
                    color = VeilText,
                )
            }
            Spacer(Modifier.height(10.dp))
            ActionButton("Copy main header", state.mainHeader.isNotBlank()) {
                copyText(context, "VeilKnit main header", state.mainHeader)
                scope.launch { snackbarHostState.showSnackbar(tr("Main header copied.")) }
            }
        }

        Spacer(Modifier.height(16.dp))
        SectionTitle("Published mailbox advertisement (subkey 2)")
        InfoCard {
            SelectionContainer {
                Text(
                    text = localizedStatus(state.mailboxHeader),
                    modifier = Modifier.fillMaxWidth(),
                    fontFamily = FontFamily.Monospace,
                    style = MaterialTheme.typography.bodySmall,
                    color = VeilText,
                )
            }
            Spacer(Modifier.height(10.dp))
            ActionButton("Copy mailbox header", state.mailboxHeader.isNotBlank()) {
                copyText(context, "VeilKnit mailbox header", state.mailboxHeader)
                scope.launch { snackbarHostState.showSnackbar(tr("Mailbox header copied.")) }
            }
        }

        Spacer(Modifier.height(12.dp))
        ActionButton("Refresh both headers", state.ready) { send(context, "headers") }

        Spacer(Modifier.height(18.dp))
        SectionTitle("Header log")
        LogPanel(state.logs.forCategory(LogCategory.Headers).takeLast(300))
    }
}

@Composable
private fun DhtPage(state: DaemonUiState, snackbarHostState: SnackbarHostState) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var name by rememberSaveable { mutableStateOf("") }
    var groups by rememberSaveable { mutableStateOf("1") }
    var index by rememberSaveable { mutableStateOf("0") }
    var subkey by rememberSaveable { mutableStateOf("0") }
    var data by rememberSaveable { mutableStateOf("") }
    var externalKey by rememberSaveable { mutableStateOf("") }
    var locations by rememberSaveable { mutableStateOf("0") }

    PageColumn {
        SectionTitle("Create owned DHT")
        SingleLineField(name, { name = it }, "DHT name")
        Spacer(Modifier.height(8.dp))
        SingleLineField(groups, { groups = it }, "Owner group sizes, comma-separated")
        Spacer(Modifier.height(8.dp))
        ActionButton("Create DHT", state.ready) {
            val parsed = groups.split(',').map { it.trim() }
            val valid = name.isNotBlank() && '\n' !in name && parsed.isNotEmpty() &&
                parsed.size <= 250 && parsed.all { token -> token.toIntOrNull()?.let { it in 1..250 } == true }
            if (!valid) {
                scope.launch {
                    snackbarHostState.showSnackbar(
                        "Enter a name and owner group sizes from 1 to 250.",
                    )
                }
            } else {
                val commands = mutableListOf("N", name.trim())
                parsed.forEachIndexed { position, group ->
                    commands += group
                    if (position < parsed.lastIndex) commands += "y"
                    else if (parsed.size < 250) commands += "n"
                }
                send(context, *commands.toTypedArray())
            }
        }

        Spacer(Modifier.height(18.dp))
        SectionTitle("Owned DHT")
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Box(Modifier.weight(1f)) { SingleLineField(index, { index = it }, "Index") }
            Box(Modifier.weight(1f)) { SingleLineField(subkey, { subkey = it }, "Subkey") }
        }
        Spacer(Modifier.height(8.dp))
        SingleLineField(data, { data = it }, "Single-line value")
        Spacer(Modifier.height(8.dp))
        TwoActionRow(
            leftText = "Inspect",
            onLeft = {
                if (index.toULongOrNull() == null) invalidNumber(scope, snackbarHostState)
                else send(context, "G", index)
            },
            rightText = "Write",
            onRight = {
                if (index.toULongOrNull() == null || subkey.toUIntOrNull() == null || '\n' in data) {
                    invalidNumber(scope, snackbarHostState, tr("Enter an index, subkey, and single-line value."))
                } else send(context, "W", index, subkey, data)
            },
            enabled = state.ready,
        )
        Spacer(Modifier.height(8.dp))
        TwoActionRow(
            leftText = "Read",
            onLeft = {
                if (index.toULongOrNull() == null || subkey.toUIntOrNull() == null) {
                    invalidNumber(scope, snackbarHostState)
                } else send(context, "R", index, subkey)
            },
            rightText = "Read all",
            onRight = {
                if (index.toULongOrNull() == null) invalidNumber(scope, snackbarHostState)
                else send(context, "L", index)
            },
            enabled = state.ready,
        )
        Spacer(Modifier.height(8.dp))
        ActionButton("Save owned DHTs", state.ready) { send(context, "S") }

        Spacer(Modifier.height(18.dp))
        SectionTitle("External DHT")
        SingleLineField(externalKey, { externalKey = it }, "External VLD0 key")
        Spacer(Modifier.height(8.dp))
        SingleLineField(locations, { locations = it }, "Subkeys, e.g. 0,1,10,50-75")
        Spacer(Modifier.height(8.dp))
        TwoActionRow(
            leftText = "Read selected",
            onLeft = {
                if (!looksLikeRecordKey(externalKey) || !looksLikeLocations(locations)) {
                    scope.launch { snackbarHostState.showSnackbar(tr("Enter a VLD0 key and valid subkeys.")) }
                } else send(context, "Y", externalKey.trim(), locations.trim())
            },
            rightText = "Read all",
            onRight = {
                if (!looksLikeRecordKey(externalKey)) {
                    scope.launch { snackbarHostState.showSnackbar(tr("Enter an external VLD0 key.")) }
                } else send(context, "X", externalKey.trim())
            },
            enabled = state.ready,
        )

        Spacer(Modifier.height(18.dp))
        SectionTitle("DHT log")
        LogPanel(state.logs.forCategory(LogCategory.Dht).takeLast(900))
    }
}

@Composable
private fun MailboxPage(state: DaemonUiState, snackbarHostState: SnackbarHostState) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var recipient by rememberSaveable { mutableStateOf("") }
    var appId by rememberSaveable { mutableStateOf("veilknit.android") }
    var payload by rememberSaveable { mutableStateOf("") }

    PageColumn {
        SectionTitle("Send mailbox message")
        SingleLineField(recipient, { recipient = it }, "Recipient VLD0 key")
        Spacer(Modifier.height(8.dp))
        SingleLineField(appId, { appId = it }, "Application id")
        Spacer(Modifier.height(8.dp))
        SingleLineField(payload, { payload = it }, "Payload")
        Spacer(Modifier.height(8.dp))
        ActionButton("Send", state.ready) {
            if (!looksLikeRecordKey(recipient) || appId.isBlank() || '\n' in appId || '\n' in payload) {
                scope.launch {
                    snackbarHostState.showSnackbar(
                        "Enter a recipient key, application id, and single-line payload.",
                    )
                }
            } else send(context, "mail send", recipient.trim(), appId.trim(), payload)
        }

        Spacer(Modifier.height(16.dp))
        TwoActionRow(
            leftText = "Status",
            onLeft = { send(context, "mail status") },
            rightText = "List inbox",
            onRight = { send(context, "mail list") },
            enabled = state.ready,
        )
        Spacer(Modifier.height(8.dp))
        TwoActionRow(
            leftText = "Retrieve",
            onLeft = { send(context, "mail retrieve") },
            rightText = "Stats",
            onRight = { send(context, "mail stats") },
            enabled = state.ready,
        )
        Spacer(Modifier.height(8.dp))
        TwoActionRow(
            leftText = "Flush",
            onLeft = { send(context, "mail flush") },
            rightText = "Repair",
            onRight = { send(context, "mail repair") },
            enabled = state.ready,
        )

        Spacer(Modifier.height(18.dp))
        SectionTitle("Mailbox log")
        LogPanel(state.logs.forCategory(LogCategory.Mailbox).takeLast(900))
    }
}

@Composable
private fun ApplicationsPage(state: DaemonUiState, snackbarHostState: SnackbarHostState) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var appId by rememberSaveable { mutableStateOf("") }
    var displayName by rememberSaveable { mutableStateOf("") }
    var checkedRequests by remember { mutableStateOf(setOf<Long>()) }
    var rejectReason by rememberSaveable { mutableStateOf("rejected by the local user") }
    var visibleName by rememberSaveable { mutableStateOf("") }
    var profileName by rememberSaveable { mutableStateOf("") }
    var profileId by rememberSaveable { mutableStateOf("") }
    var advanced by rememberSaveable { mutableStateOf(false) }

    LaunchedEffect(state.pendingAppRequests) {
        val valid = state.pendingAppRequests.map { it.requestId }.toSet()
        checkedRequests = checkedRequests.intersect(valid)
    }

    PageColumn {
        SectionTitle("Registration requests")
        Text(
            tr("Only the newest request for each application is shown. Check the requests you want to act on."),
            color = VeilMuted,
            style = MaterialTheme.typography.bodySmall,
        )
        Spacer(Modifier.height(8.dp))
        ActionButton("Refresh requests", state.ready) { send(context, "app-pending") }
        Spacer(Modifier.height(8.dp))
        if (state.pendingAppRequests.isEmpty()) {
            Text(tr("No pending application requests."), color = VeilMuted)
        } else {
            state.pendingAppRequests.forEach { request ->
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(vertical = 4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Checkbox(
                        checked = request.requestId in checkedRequests,
                        onCheckedChange = { checked ->
                            checkedRequests = if (checked) {
                                checkedRequests + request.requestId
                            } else {
                                checkedRequests - request.requestId
                            }
                        },
                    )
                    Column(Modifier.weight(1f)) {
                        Text(request.appId, color = VeilText, fontWeight = FontWeight.SemiBold)
                        Text(
                            if (request.displayName.isBlank()) "#${request.requestId}"
                            else "${request.displayName}  •  #${request.requestId}",
                            color = VeilMuted,
                            style = MaterialTheme.typography.bodySmall,
                        )
                    }
                }
                HorizontalDivider(color = VeilBorder)
            }
        }
        Spacer(Modifier.height(8.dp))
        TwoActionRow(
            leftText = "Allow checked",
            onLeft = {
                val commands = checkedRequests.sorted().map { "app-approve $it" } + "app-pending"
                if (checkedRequests.isNotEmpty()) send(context, *commands.toTypedArray())
            },
            rightText = "Refuse checked",
            onRight = {
                val reason = rejectReason.ifBlank { "rejected by the local user" }
                val commands = checkedRequests.sorted().map { "app-reject $it $reason" } + "app-pending"
                if (checkedRequests.isNotEmpty() && '\n' !in reason) send(context, *commands.toTypedArray())
            },
            enabled = state.ready && checkedRequests.isNotEmpty(),
        )

        Spacer(Modifier.height(18.dp))
        SectionTitle("Observed applications")
        if (state.foundApps.isEmpty()) {
            Text(tr("No application advertisements have been observed yet."), color = VeilMuted)
        } else {
            state.foundApps.forEach { app ->
                Card(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(vertical = 3.dp),
                    colors = CardDefaults.cardColors(containerColor = VeilPanel),
                ) {
                    Column(Modifier.padding(10.dp)) {
                        Text(app.appId, color = VeilText, fontWeight = FontWeight.SemiBold)
                        Text(
                            tr("Verified headers") + ": ${app.observedHeaders}   •   " +
                                tr("Discovery cache") + ": ${app.discoveryCache}",
                            color = VeilMuted,
                            style = MaterialTheme.typography.bodySmall,
                        )
                        if (app.discoveryCache > 0) {
                            Text(
                                tr("Recent") + ": ${app.recent}   •   " + tr("Archive") + ": ${app.archive}",
                                color = VeilMuted,
                                style = MaterialTheme.typography.bodySmall,
                            )
                        }
                    }
                }
            }
        }

        Spacer(Modifier.height(10.dp))
        TextButton(onClick = { advanced = !advanced }) {
            Icon(
                if (advanced) Icons.Default.ExpandLess else Icons.Default.ExpandMore,
                contentDescription = null,
            )
            Spacer(Modifier.width(6.dp))
            Text(tr("Advanced application management"))
        }

        if (advanced) {
            // Refresh the registered list whenever the panel is opened, so the rows reflect
            // what the daemon currently holds rather than whatever was last typed.
            LaunchedEffect(advanced, state.ready) {
                if (state.ready) send(context, "app-list")
            }

            Spacer(Modifier.height(10.dp))
            SectionTitle("Registered applications")
            if (state.localApps.isEmpty()) {
                Text(
                    tr("No applications are registered on this daemon yet."),
                    color = VeilMuted,
                    style = MaterialTheme.typography.bodySmall,
                )
            } else {
                state.localApps.forEach { app ->
                    val selected = appId.trim() == app.appId
                    Card(
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(vertical = 3.dp)
                            .clickable {
                                // Selecting fills the fields below, so rotating or renaming
                                // never requires retyping an app id by hand.
                                appId = app.appId
                                displayName = app.displayName
                            },
                        colors = CardDefaults.cardColors(
                            containerColor = if (selected) VeilEdit else VeilPanel,
                        ),
                    ) {
                        Column(Modifier.padding(10.dp)) {
                            Text(
                                app.displayName.ifBlank { app.appId },
                                color = VeilText,
                                fontWeight = FontWeight.SemiBold,
                            )
                            Text(app.appId, color = VeilMuted, style = MaterialTheme.typography.bodySmall)
                            Text(
                                tr("Key generation") + ": ${app.credentialGeneration}   |   " +
                                    tr("Capabilities") + ": ${app.capabilityCount}" +
                                    if (app.enabled) "" else "   |   " + tr("disabled"),
                                color = VeilMuted,
                                style = MaterialTheme.typography.bodySmall,
                            )
                            if (selected) {
                                Spacer(Modifier.height(8.dp))
                                ActionButton(
                                    tr("Rotate this app's key"),
                                    state.ready,
                                ) { send(context, "app-rotate", app.appId) }
                                Text(
                                    tr("Use this after reinstalling an app. It clears the old credential so the app can register again."),
                                    color = VeilMuted,
                                    style = MaterialTheme.typography.bodySmall,
                                )
                            }
                        }
                    }
                }
            }

            Spacer(Modifier.height(14.dp))
            SectionTitle("Local applications")
            SingleLineField(appId, { appId = it }, "Application id")
            Spacer(Modifier.height(8.dp))
            SingleLineField(displayName, { displayName = it }, "Display name")
            Spacer(Modifier.height(8.dp))
            TwoActionRow(
                leftText = "Register",
                onLeft = {
                    if (appId.isBlank() || displayName.isBlank() || '\n' in appId || '\n' in displayName) {
                        scope.launch { snackbarHostState.showSnackbar(tr("Enter an application id and display name.")) }
                    } else send(context, "app-add", appId.trim(), displayName.trim())
                },
                rightText = "List",
                onRight = { send(context, "app-list") },
                enabled = state.ready,
            )
            Spacer(Modifier.height(8.dp))
            ActionButton("Rotate selected app key", state.ready && appId.isNotBlank()) {
                send(context, "app-rotate", appId.trim())
            }

            Spacer(Modifier.height(18.dp))
            SectionTitle("Refusal reason")
            SingleLineField(rejectReason, { rejectReason = it }, "Reason used by Refuse checked")

            Spacer(Modifier.height(18.dp))
            SectionTitle("Names shown to applications")
            SingleLineField(visibleName, { visibleName = it }, "Visible name")
            Spacer(Modifier.height(8.dp))
            TwoActionRow(
                leftText = "Set default name",
                onLeft = {
                    if (visibleName.isBlank() || '\n' in visibleName) {
                        scope.launch { snackbarHostState.showSnackbar(tr("Enter a single-line visible name.")) }
                    } else send(context, "app-name default ${visibleName.trim()}")
                },
                rightText = "List name settings",
                onRight = { send(context, "app-name") },
                enabled = state.ready,
            )
            Spacer(Modifier.height(8.dp))
            TwoActionRow(
                leftText = "Set selected app alias",
                onLeft = {
                    if (appId.isBlank() || visibleName.isBlank() || '\n' in appId || '\n' in visibleName) {
                        scope.launch { snackbarHostState.showSnackbar(tr("Enter an application id and visible name.")) }
                    } else send(context, "app-name set ${appId.trim()} ${visibleName.trim()}")
                },
                rightText = "Clear selected alias",
                onRight = {
                    if (appId.isBlank() || '\n' in appId) {
                        scope.launch { snackbarHostState.showSnackbar(tr("Enter an application id.")) }
                    } else send(context, "app-name clear ${appId.trim()}")
                },
                enabled = state.ready,
            )

            Spacer(Modifier.height(18.dp))
            SectionTitle("Network profiles")
            SingleLineField(profileName, { profileName = it }, "New profile name")
            Spacer(Modifier.height(8.dp))
            TwoActionRow(
                leftText = "Create profile",
                onLeft = {
                    if (profileName.isBlank() || '\n' in profileName) {
                        scope.launch { snackbarHostState.showSnackbar(tr("Enter a single-line profile name.")) }
                    } else send(context, "profile-create ${profileName.trim()}")
                },
                rightText = "List profiles",
                onRight = { send(context, "profile-list") },
                enabled = state.ready,
            )
            Spacer(Modifier.height(8.dp))
            SingleLineField(profileId, { profileId = it }, "Profile id")
            Spacer(Modifier.height(8.dp))
            TwoActionRow(
                leftText = "Use after restart",
                onLeft = {
                    if (profileId.isBlank() || '\n' in profileId) {
                        scope.launch { snackbarHostState.showSnackbar(tr("Enter a profile id.")) }
                    } else send(context, "profile-use ${profileId.trim()}")
                },
                rightText = "Retire profile",
                onRight = {
                    if (profileId.isBlank() || '\n' in profileId) {
                        scope.launch { snackbarHostState.showSnackbar(tr("Enter a profile id.")) }
                    } else send(context, "profile-retire ${profileId.trim()}")
                },
                enabled = state.ready,
            )
            Text(
                tr("Profile changes take effect after a controlled daemon restart."),
                color = VeilMuted,
                style = MaterialTheme.typography.bodySmall,
            )
        }

        Spacer(Modifier.height(18.dp))
        SectionTitle("Application log")
        LogPanel(state.logs.forCategory(LogCategory.Applications).takeLast(900))
    }
}

@Composable
private fun BackupPage(state: DaemonUiState, snackbarHostState: SnackbarHostState) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var passphrase by rememberSaveable { mutableStateOf("") }
    var passphraseVisible by rememberSaveable { mutableStateOf(false) }
    var recoveryCode by rememberSaveable { mutableStateOf("") }
    var lastPath by rememberSaveable { mutableStateOf("") }

    fun newBackupPath(prefix: String): String {
        val root = context.getExternalFilesDir(Environment.DIRECTORY_DOCUMENTS)
            ?: context.filesDir
        val directory = File(root, "VeilKnit Backups").apply { mkdirs() }
        return File(directory, "$prefix-${System.currentTimeMillis()}.veilknit-backup").absolutePath
    }

    fun requirePassphrase(action: (String) -> Unit) {
        if (passphrase.length < 8 || '\n' in passphrase || '\r' in passphrase) {
            scope.launch {
                snackbarHostState.showSnackbar(tr("Use a backup passphrase of at least 8 characters."))
            }
        } else {
            action(passphrase)
        }
    }

    PageColumn {
        SectionTitle("Encrypted identity backup")
        Text(
            tr("Backups exclude logs and regenerable routing caches. Move the exported file out of the app folder before uninstalling."),
            color = VeilMuted,
            style = MaterialTheme.typography.bodySmall,
        )
        Spacer(Modifier.height(10.dp))
        OutlinedTextField(
            value = passphrase,
            onValueChange = { passphrase = it },
            label = { Text(tr("Backup passphrase")) },
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
            visualTransformation = if (passphraseVisible) VisualTransformation.None else PasswordVisualTransformation(),
            trailingIcon = {
                IconButton(onClick = { passphraseVisible = !passphraseVisible }) {
                    Icon(
                        if (passphraseVisible) Icons.Default.VisibilityOff else Icons.Default.Visibility,
                        contentDescription = tr(if (passphraseVisible) "Hide password" else "Show password"),
                    )
                }
            },
        )
        Spacer(Modifier.height(8.dp))
        ActionButton("Create local backup", state.ready) {
            requirePassphrase { secret ->
                val path = newBackupPath("veilknit-local")
                lastPath = path
                send(context, "backup-local $path", secret, secret)
                scope.launch { snackbarHostState.showSnackbar(tr("Backup creation was queued. Watch the backup log for completion.")) }
            }
        }
        Spacer(Modifier.height(8.dp))
        ActionButton("Create backup and upload recovery copy", state.ready) {
            requirePassphrase { secret ->
                val path = newBackupPath("veilknit-network-source")
                lastPath = path
                send(context, "backup-local $path", secret, secret, "recovery-upload $path")
                scope.launch { snackbarHostState.showSnackbar(tr("Backup and network recovery upload were queued. Save the recovery code shown in the log.")) }
            }
        }
        if (lastPath.isNotBlank()) {
            Spacer(Modifier.height(8.dp))
            SelectionContainer { Text(lastPath, fontFamily = FontFamily.Monospace, color = VeilMuted) }
            TextButton(onClick = { copyText(context, "VeilKnit backup path", lastPath) }) {
                Icon(Icons.Default.ContentCopy, contentDescription = null)
                Spacer(Modifier.width(6.dp))
                Text(tr("Copy backup path"))
            }
        }

        Spacer(Modifier.height(18.dp))
        SectionTitle("Network recovery")
        Text(
            tr("The recovery code contains the random DHT address and decryption secret. Store it separately from the backup passphrase."),
            color = VeilMuted,
            style = MaterialTheme.typography.bodySmall,
        )
        Spacer(Modifier.height(8.dp))
        SingleLineField(recoveryCode, { recoveryCode = it }, "VKR1 recovery code")
        Spacer(Modifier.height(8.dp))
        ActionButton("Download recovery backup", state.ready && recoveryCode.trim().startsWith("VKR1|")) {
            val path = newBackupPath("veilknit-recovered")
            lastPath = path
            send(context, "recovery-download ${recoveryCode.trim()} $path")
        }
        Spacer(Modifier.height(8.dp))
        TwoActionRow(
            leftText = "Recovery status",
            onLeft = { send(context, "recovery-status") },
            rightText = "Wipe network recovery",
            onRight = { send(context, "recovery-wipe", "WIPE") },
            enabled = state.ready,
        )

        Spacer(Modifier.height(18.dp))
        SectionTitle("Backup and recovery log")
        LogPanel(
            state.logs.filter {
                it.contains("backup", ignoreCase = true) ||
                    it.contains("recovery", ignoreCase = true)
            }.takeLast(700),
        )
    }
}

@Composable
private fun LogsPage(logs: List<String>) {
    Column(
        Modifier
            .fillMaxSize()
            .padding(12.dp),
    ) {
        Text(
            "All daemon logs",
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.Bold,
        )
        Spacer(Modifier.height(8.dp))
        LogPanel(logs.takeLast(2_000), modifier = Modifier.fillMaxSize())
    }
}

@Composable
private fun PageColumn(content: @Composable ColumnScope.() -> Unit) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(14.dp),
        content = content,
    )
}

@Composable
private fun SectionTitle(text: String) {
    Text(
        text = tr(text),
        style = MaterialTheme.typography.titleMedium,
        fontWeight = FontWeight.Bold,
        color = VeilText,
    )
    Spacer(Modifier.height(8.dp))
}

@Composable
private fun InfoCard(content: @Composable ColumnScope.() -> Unit) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(containerColor = VeilPanel),
        shape = RoundedCornerShape(14.dp),
    ) {
        Column(Modifier.padding(14.dp), content = content)
    }
}

@Composable
private fun WarningCard(text: String) {
    Card(
        modifier = Modifier
            .fillMaxWidth()
            .border(1.dp, VeilRed, RoundedCornerShape(12.dp)),
        colors = CardDefaults.cardColors(containerColor = VeilPanel),
        shape = RoundedCornerShape(12.dp),
    ) {
        Text(
            text = text,
            modifier = Modifier.padding(12.dp),
            color = VeilText,
            style = MaterialTheme.typography.bodySmall,
        )
    }
}

@Composable
private fun SingleLineField(
    value: String,
    onValueChange: (String) -> Unit,
    label: String,
) {
    OutlinedTextField(
        value = value,
        onValueChange = onValueChange,
        label = { Text(tr(label)) },
        singleLine = true,
        modifier = Modifier.fillMaxWidth(),
    )
}

@Composable
private fun ActionButton(text: String, enabled: Boolean = true, onClick: () -> Unit) {
    Button(
        onClick = onClick,
        enabled = enabled,
        modifier = Modifier.fillMaxWidth(),
        colors = ButtonDefaults.buttonColors(containerColor = VeilRed),
    ) {
        Text(tr(text))
    }
}

@Composable
private fun TwoActionRow(
    leftText: String,
    onLeft: () -> Unit,
    rightText: String,
    onRight: () -> Unit,
    enabled: Boolean = true,
    leftIcon: (@Composable () -> Unit)? = null,
    rightIcon: (@Composable () -> Unit)? = null,
) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Button(
            onClick = onLeft,
            enabled = enabled,
            modifier = Modifier.weight(1f),
            colors = ButtonDefaults.buttonColors(containerColor = VeilRed),
            contentPadding = PaddingValues(horizontal = 8.dp, vertical = 10.dp),
        ) {
            leftIcon?.invoke()
            if (leftIcon != null) Spacer(Modifier.width(6.dp))
            Text(tr(leftText), maxLines = 1)
        }
        Button(
            onClick = onRight,
            enabled = enabled,
            modifier = Modifier.weight(1f),
            colors = ButtonDefaults.buttonColors(containerColor = VeilRed),
            contentPadding = PaddingValues(horizontal = 8.dp, vertical = 10.dp),
        ) {
            rightIcon?.invoke()
            if (rightIcon != null) Spacer(Modifier.width(6.dp))
            Text(tr(rightText), maxLines = 1)
        }
    }
}

@Composable
private fun LogPanel(lines: List<String>, modifier: Modifier = Modifier) {
    val shown = if (lines.isEmpty()) listOf(tr("No matching log lines yet.")) else lines
    LazyColumn(
        modifier = modifier
            .fillMaxWidth()
            .heightIn(min = 180.dp, max = 520.dp)
            .background(VeilEdit, RoundedCornerShape(10.dp))
            .border(1.dp, VeilBorder, RoundedCornerShape(10.dp))
            .padding(10.dp),
        verticalArrangement = Arrangement.spacedBy(3.dp),
    ) {
        items(shown) { line ->
            SelectionContainer {
                Text(
                    text = line,
                    fontFamily = FontFamily.Monospace,
                    style = MaterialTheme.typography.bodySmall,
                    color = if (lines.isEmpty()) VeilMuted else VeilText,
                )
            }
        }
    }
}

private fun send(context: Context, vararg commands: String) {
    DaemonForegroundService.sendCommands(context, commands.toList())
}

private fun copyText(context: Context, label: String, value: String) {
    val clipboard = context.getSystemService(ClipboardManager::class.java)
    clipboard.setPrimaryClip(ClipData.newPlainText(label, value))
}

private fun looksLikeRecordKey(value: String): Boolean =
    value.trim().startsWith("VLD0:") && value.trim().length > 12 && '\n' !in value

private fun looksLikeLocations(value: String): Boolean =
    Regex("^\\d+(?:-\\d+)?(?:,\\d+(?:-\\d+)?)*$").matches(value.trim())

private fun localizedStatus(value: String): String = when (value) {
    "Stopped", "Starting…", "Running", "Authenticated; starting network services…",
    "Authentication failed", "Error", "Waiting for the first header read…" -> tr(value)
    else -> value
}


private fun invalidNumber(
    scope: kotlinx.coroutines.CoroutineScope,
    host: SnackbarHostState,
    message: String = "Enter valid numeric index and subkey values.",
) {
    scope.launch { host.showSnackbar(message) }
}
