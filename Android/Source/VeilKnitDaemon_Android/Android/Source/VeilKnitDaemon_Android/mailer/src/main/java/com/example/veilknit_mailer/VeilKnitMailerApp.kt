package com.example.veilknit_mailer

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.*
import java.text.DateFormat
import java.util.Date

private enum class MailerTab(val title: String) { Contacts("Contacts"), Inbox("Inbox"), Compose("Compose") }

private class MailerController(
    private val api: MailerApiClient,
    private val nicknameStore: NicknameStore,
) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private var pollJob: Job? = null
    var status by mutableStateOf(tr("Connecting to VeilKnit Daemon…"))
    var authorizationRequest by mutableStateOf<Long?>(null)
    var ready by mutableStateOf(false)
    var busy by mutableStateOf(false)
    var contacts by mutableStateOf<List<KnownNode>>(emptyList())
    var inbox by mutableStateOf<List<InboxSummary>>(emptyList())
    var nicknames by mutableStateOf(nicknameStore.load())
    var selectedMessage by mutableStateOf<InboxMessage?>(null)
    var selectedRecipient by mutableStateOf<String?>(null)
    var draft by mutableStateOf("")

    fun start() {
        scope.launch {
            try {
                withContext(Dispatchers.IO) { api.connectAndAuthenticate() }
                authorizationRequest = null
                ready = true
                status = tr("Connected")
                refreshAll()
                beginPolling()
            } catch (pending: AuthorizationPending) {
                authorizationRequest = pending.requestId
                status = "${tr("Authorization required")} #${pending.requestId}"
            } catch (error: Throwable) {
                ready = false
                status = error.message ?: error.javaClass.simpleName
            }
        }
    }

    fun checkApproval() = launchBusy {
        withContext(Dispatchers.IO) { api.checkAuthorizationAndAuthenticate() }
        authorizationRequest = null
        ready = true
        status = tr("Connected")
        refreshAll()
        beginPolling()
    }

    fun freshConnect() {
        pollJob?.cancel()
        scope.launch {
            busy = true
            ready = false
            authorizationRequest = null
            contacts = emptyList()
            inbox = emptyList()
            selectedMessage = null
            selectedRecipient = null
            status = tr("Connecting to the current daemon account…")
            try {
                withContext(Dispatchers.IO) { api.freshConnect() }
                ready = true
                status = tr("Connected")
                refreshAll()
                beginPolling()
            } catch (pending: AuthorizationPending) {
                authorizationRequest = pending.requestId
                status = "${tr("Authorization required")} #${pending.requestId}"
            } catch (error: Throwable) {
                status = error.message ?: error.javaClass.simpleName
            } finally {
                busy = false
            }
        }
    }

    fun refreshAll() = scope.launch {
        refreshContacts(silent = true)
        refreshInbox(silent = true)
    }

    fun refreshContacts(silent: Boolean = false) = scope.launch {
        if (!ready) return@launch
        if (!silent) busy = true
        try {
            contacts = withContext(Dispatchers.IO) { api.listKnownNodes() }
            status = "${contacts.size} ${tr("known node(s)")}"
        } catch (error: Throwable) { status = error.message ?: tr("Could not load contacts") }
        finally { if (!silent) busy = false }
    }

    fun refreshInbox(silent: Boolean = false) = scope.launch {
        if (!ready) return@launch
        if (!silent) busy = true
        try {
            inbox = withContext(Dispatchers.IO) { api.listInbox() }
            status = "${inbox.count { !it.read }} ${tr("unread message(s)")}"
        } catch (error: Throwable) { status = error.message ?: tr("Could not load inbox") }
        finally { if (!silent) busy = false }
    }

    fun retrieveMail() = launchBusy {
        withContext(Dispatchers.IO) { api.triggerRetrieval() }
        status = tr("Mailbox retrieval requested")
        delay(1200)
        inbox = withContext(Dispatchers.IO) { api.listInbox() }
    }

    fun openMessage(messageId: String) = launchBusy {
        selectedMessage = withContext(Dispatchers.IO) { api.readInbox(messageId) }
        inbox = withContext(Dispatchers.IO) { api.listInbox() }
    }

    fun deleteMessage(messageId: String) = launchBusy {
        withContext(Dispatchers.IO) { api.deleteInbox(messageId) }
        selectedMessage = null
        inbox = withContext(Dispatchers.IO) { api.listInbox() }
        status = tr("Message deleted")
    }

    fun send() = launchBusy {
        val recipient = selectedRecipient ?: error(tr("Select a recipient"))
        val text = draft.trim()
        require(text.isNotEmpty()) { tr("Type a message first") }
        val messageId = withContext(Dispatchers.IO) { api.sendMessage(recipient, text) }
        draft = ""
        status = tr("Mail queued") + ": ${messageId.take(12)}…"
    }

    fun setNickname(key: String, value: String) {
        val updated = nicknames.toMutableMap()
        if (value.isBlank()) updated.remove(key) else updated[key] = value.trim()
        nicknames = updated
        nicknameStore.save(updated)
    }

    fun displayName(key: String): String = nicknames[key] ?: shortKey(key)

    fun languageChanged() {
        status = when {
            authorizationRequest != null -> "${tr("Authorization required")} #${authorizationRequest}"
            ready -> tr("Connected")
            else -> tr("Connecting to VeilKnit Daemon…")
        }
    }

    fun close() { scope.cancel(); api.close() }

    private fun beginPolling() {
        pollJob?.cancel()
        pollJob = scope.launch {
            while (isActive) {
                delay(20_000)
                refreshInbox(silent = true)
            }
        }
    }

    private fun launchBusy(block: suspend () -> Unit) = scope.launch {
        busy = true
        try { block() } catch (error: Throwable) { status = error.message ?: error.javaClass.simpleName }
        finally { busy = false }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun VeilKnitMailerApp() {
    val context = LocalContext.current
    var language by remember { mutableStateOf(LanguagePreferences.load(context)) }
    UiStrings.current = language
    val controller = remember { MailerController(MailerApiClient(context), NicknameStore(context)) }
    var tab by remember { mutableStateOf(MailerTab.Contacts) }
    var nicknameTarget by remember { mutableStateOf<KnownNode?>(null) }
    DisposableEffect(Unit) { controller.start(); onDispose(controller::close) }

    val colors = darkColorScheme(
        primary = Color(0xFFEF233C),
        background = Color(0xFF131315),
        surface = Color(0xFF1C1C1F),
        surfaceVariant = Color(0xFF29292E),
        onBackground = Color(0xFFEBEBEE),
        onSurface = Color(0xFFEBEBEE),
    )
    MaterialTheme(colorScheme = colors) {
        Scaffold(
            topBar = {
                TopAppBar(
                    title = { Column { Text(tr("VeilKnit Mailer")); Text(controller.status, style = MaterialTheme.typography.labelSmall, maxLines = 1, overflow = TextOverflow.Ellipsis) } },
                    actions = {
                        TextButton(
                            onClick = controller::freshConnect,
                            enabled = !controller.busy,
                        ) {
                            Icon(Icons.Default.Sync, contentDescription = null)
                            Spacer(Modifier.width(4.dp))
                            Text(tr("Fresh connect"))
                        }
                        LanguageSelector(
                            language = language,
                            onLanguageChange = {
                                language = it
                                UiStrings.current = it
                                LanguagePreferences.save(context, it)
                                controller.languageChanged()
                            },
                        )
                        if (controller.busy) CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp)
                    }
                )
            },
            bottomBar = {
                NavigationBar {
                    MailerTab.entries.forEach { item ->
                        NavigationBarItem(
                            selected = tab == item,
                            onClick = { tab = item },
                            icon = { Icon(when (item) { MailerTab.Contacts -> Icons.Default.People; MailerTab.Inbox -> Icons.Default.Inbox; MailerTab.Compose -> Icons.Default.Edit }, null) },
                            label = { Text(tr(item.title)) },
                        )
                    }
                }
            },
        ) { padding ->
            Box(Modifier.padding(padding).fillMaxSize()) {
                when {
                    controller.authorizationRequest != null -> AuthorizationPanel(controller.authorizationRequest!!, controller::checkApproval)
                    !controller.ready -> ConnectionPanel(controller.status, controller::start)
                    tab == MailerTab.Contacts -> ContactsPanel(controller, onCompose = { node -> controller.selectedRecipient = node.mainDht; tab = MailerTab.Compose }, onNickname = { nicknameTarget = it })
                    tab == MailerTab.Inbox -> InboxPanel(controller)
                    else -> ComposePanel(controller)
                }
            }
        }
    }

    nicknameTarget?.let { target ->
        NicknameDialog(
            initial = controller.nicknames[target.mainDht].orEmpty(),
            onDismiss = { nicknameTarget = null },
            onSave = { controller.setNickname(target.mainDht, it); nicknameTarget = null },
        )
    }
    controller.selectedMessage?.let { message ->
        AlertDialog(
            onDismissRequest = { controller.selectedMessage = null },
            title = { Text(tr("From") + " ${controller.displayName(message.senderMainDht)}") },
            text = { Column { Text(formatDate(message.receivedAt), style = MaterialTheme.typography.labelMedium); Spacer(Modifier.height(12.dp)); Text(message.text) } },
            confirmButton = { TextButton(onClick = { controller.selectedMessage = null }) { Text(tr("Close")) } },
            dismissButton = { TextButton(onClick = { controller.deleteMessage(message.messageId) }) { Text(tr("Delete")) } },
        )
    }
}

@Composable private fun AuthorizationPanel(id: Long, check: () -> Unit) = CenteredPanel {
    Icon(Icons.Default.AdminPanelSettings, null, Modifier.size(56.dp))
    Text(tr("Mailer authorization required"), style = MaterialTheme.typography.titleLarge)
    Text(tr("Open VeilKnit Daemon, approve application request") + " #$id; " + tr("then return here."))
    Button(onClick = check) { Text(tr("Check approval")) }
}

@Composable private fun ConnectionPanel(status: String, retry: () -> Unit) = CenteredPanel {
    Icon(Icons.Default.CloudOff, null, Modifier.size(56.dp))
    Text(status)
    Button(onClick = retry) { Text(tr("Retry")) }
}

@Composable private fun CenteredPanel(content: @Composable ColumnScope.() -> Unit) {
    Column(Modifier.fillMaxSize().padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.Center, content = content)
}

@Composable private fun ContactsPanel(controller: MailerController, onCompose: (KnownNode) -> Unit, onNickname: (KnownNode) -> Unit) {
    Column(Modifier.fillMaxSize()) {
        Row(Modifier.fillMaxWidth().padding(12.dp), horizontalArrangement = Arrangement.End) {
            OutlinedButton(onClick = { controller.refreshContacts() }) { Icon(Icons.Default.Refresh, null); Spacer(Modifier.width(6.dp)); Text(tr("Refresh")) }
        }
        if (controller.contacts.isEmpty()) CenteredPanel { Text(tr("No known nodes yet. Let the daemon complete a walk.")) }
        else LazyColumn(Modifier.fillMaxSize()) {
            items(controller.contacts, key = { it.mainDht }) { node ->
                ListItem(
                    headlineContent = { Text(controller.displayName(node.mainDht), fontWeight = if (node.verified) FontWeight.SemiBold else FontWeight.Normal) },
                    supportingContent = { Text("${localizedNodeState(node.presenceState)} • ${localizedNodeState(node.verificationState)}\n${node.mainDht}", maxLines = 3, overflow = TextOverflow.Ellipsis) },
                    leadingContent = { Icon(if (node.mailboxCapable) Icons.Default.MarkEmailRead else Icons.Default.Person, null) },
                    trailingContent = { IconButton(onClick = { onNickname(node) }) { Icon(Icons.Default.DriveFileRenameOutline, tr("Nickname")) } },
                    modifier = Modifier.fillMaxWidth(),
                )
                Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp), horizontalArrangement = Arrangement.End) {
                    TextButton(onClick = { onCompose(node) }) { Text(tr("Write mail")) }
                }
                HorizontalDivider()
            }
        }
    }
}

@Composable private fun InboxPanel(controller: MailerController) {
    Column(Modifier.fillMaxSize()) {
        Row(Modifier.fillMaxWidth().padding(12.dp), horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End)) {
            OutlinedButton(onClick = controller::retrieveMail) { Icon(Icons.Default.CloudDownload, null); Spacer(Modifier.width(6.dp)); Text(tr("Find mail")) }
            OutlinedButton(onClick = { controller.refreshInbox() }) { Icon(Icons.Default.Refresh, null); Spacer(Modifier.width(6.dp)); Text(tr("Refresh")) }
        }
        if (controller.inbox.isEmpty()) CenteredPanel { Text(tr("Your Mailer inbox is empty.")) }
        else LazyColumn(Modifier.fillMaxSize()) {
            items(controller.inbox, key = { it.messageId }) { message ->
                ListItem(
                    headlineContent = { Text(controller.displayName(message.senderMainDht), fontWeight = if (!message.read) FontWeight.Bold else FontWeight.Normal) },
                    supportingContent = { Text("${formatDate(message.receivedAt)} • ${message.plaintextLength} bytes") },
                    leadingContent = { Icon(if (message.read) Icons.Default.Drafts else Icons.Default.MarkEmailUnread, null) },
                    modifier = Modifier.fillMaxWidth(),
                )
                Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp), horizontalArrangement = Arrangement.End) {
                    TextButton(onClick = { controller.openMessage(message.messageId) }) { Text(tr("Open")) }
                }
                HorizontalDivider()
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable private fun ComposePanel(controller: MailerController) {
    var expanded by remember { mutableStateOf(false) }
    Column(Modifier.fillMaxSize().padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        ExposedDropdownMenuBox(expanded = expanded, onExpandedChange = { expanded = it }) {
            OutlinedTextField(
                value = controller.selectedRecipient?.let(controller::displayName).orEmpty(),
                onValueChange = {}, readOnly = true, label = { Text(tr("Recipient")) },
                trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded) },
                modifier = Modifier.menuAnchor(MenuAnchorType.PrimaryNotEditable).fillMaxWidth(),
            )
            ExposedDropdownMenu(expanded = expanded, onDismissRequest = { expanded = false }) {
                controller.contacts.forEach { node ->
                    DropdownMenuItem(
                        text = { Column { Text(controller.displayName(node.mainDht)); Text(shortKey(node.mainDht), style = MaterialTheme.typography.labelSmall) } },
                        onClick = { controller.selectedRecipient = node.mainDht; expanded = false },
                    )
                }
            }
        }
        OutlinedTextField(
            value = controller.draft,
            onValueChange = { controller.draft = it },
            label = { Text(tr("Message")) },
            minLines = 8,
            modifier = Modifier.fillMaxWidth().weight(1f),
        )
        Text("${controller.draft.toByteArray().size} / 8192 bytes", style = MaterialTheme.typography.labelSmall)
        Button(onClick = controller::send, enabled = controller.selectedRecipient != null && controller.draft.isNotBlank(), modifier = Modifier.align(Alignment.End)) {
            Icon(Icons.Default.Send, null); Spacer(Modifier.width(8.dp)); Text(tr("Send"))
        }
    }
}

@Composable private fun NicknameDialog(initial: String, onDismiss: () -> Unit, onSave: (String) -> Unit) {
    var value by remember(initial) { mutableStateOf(initial) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(tr("Contact nickname")) },
        text = { OutlinedTextField(value = value, onValueChange = { value = it }, label = { Text(tr("Nickname")) }, singleLine = true) },
        confirmButton = { TextButton(onClick = { onSave(value) }) { Text(tr("Save")) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(tr("Cancel")) } },
    )
}

private fun localizedNodeState(value: String): String = when (value.trim().lowercase()) {
    "online" -> tr("Online")
    "explicitly offline", "explicitly_offline" -> tr("Explicitly offline")
    "stale online claim", "stale_online_claim" -> tr("Stale online claim")
    "needs refresh", "needs_refresh" -> tr("Needs refresh")
    "unknown" -> tr("Unknown")
    "verified" -> tr("Verified")
    "unverified" -> tr("Unverified")
    else -> value
}

private fun shortKey(value: String): String = if (value.length <= 24) value else "${value.take(13)}…${value.takeLast(8)}"
private fun formatDate(seconds: Long): String = DateFormat.getDateTimeInstance(DateFormat.SHORT, DateFormat.SHORT).format(Date(seconds * 1000))
