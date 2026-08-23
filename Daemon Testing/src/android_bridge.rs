//! JNI bridge for the Android foreground service.
//!
//! The existing daemon is deliberately kept command-compatible with the
//! desktop GUI. Android sends the same newline-oriented commands through a
//! channel, while log lines are buffered for Kotlin to poll.

use std::{
    collections::VecDeque,
    fs,
    panic::{self, AssertUnwindSafe},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
        Mutex, OnceLock,
    },
    thread,
};

use jni::{
    errors::LogErrorAndDefault,
    objects::{JClass, JObject, JString},
    sys::{jboolean, jstring, JNI_FALSE, JNI_TRUE},
    EnvUnowned,
};

const MAX_PENDING_LOG_LINES: usize = 20_000;

static COMMAND_SENDER: OnceLock<Mutex<Option<Sender<String>>>> = OnceLock::new();
static COMMAND_RECEIVER: OnceLock<Mutex<Option<Receiver<String>>>> = OnceLock::new();
static LOG_QUEUE: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();
static RUNNING: AtomicBool = AtomicBool::new(false);
static STOP_REQUESTED: AtomicBool = AtomicBool::new(false);
static COMMAND_LOOP_READY: AtomicBool = AtomicBool::new(false);
const STOP_SENTINEL: &str = "__VEILKNIT_ANDROID_STOP__";

fn command_sender() -> &'static Mutex<Option<Sender<String>>> {
    COMMAND_SENDER.get_or_init(|| Mutex::new(None))
}

fn command_receiver() -> &'static Mutex<Option<Receiver<String>>> {
    COMMAND_RECEIVER.get_or_init(|| Mutex::new(None))
}

fn log_queue() -> &'static Mutex<VecDeque<String>> {
    LOG_QUEUE.get_or_init(|| Mutex::new(VecDeque::new()))
}

pub(crate) fn stop_requested() -> bool {
    STOP_REQUESTED.load(Ordering::SeqCst)
}

/// Android currently does not forward ConnectivityManager change callbacks
/// into the Rust bridge. Keep a stable generation until that callback is
/// explicitly wired, so the network wait loop can still compile and operate.
pub(crate) fn network_change_generation() -> u64 {
    0
}

pub(crate) fn network_description() -> &'static str {
    "Android network (bridge callback unavailable)"
}

pub(crate) fn publish_log(line: &str) {
    let mut queue = log_queue()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    queue.push_back(line.to_owned());
    while queue.len() > MAX_PENDING_LOG_LINES {
        queue.pop_front();
    }
}

pub(crate) fn read_command() -> String {
    loop {
        if STOP_REQUESTED.load(Ordering::Relaxed) {
            return STOP_SENTINEL.to_string();
        }

        let received = {
            let guard = command_receiver()
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            guard.as_ref().and_then(|receiver| receiver.recv().ok())
        };

        match received {
            Some(command) => return command,
            None => return "Q".to_string(),
        }
    }
}


pub(crate) fn mark_command_loop_ready() {
    COMMAND_LOOP_READY.store(true, Ordering::SeqCst);
}

pub(crate) fn is_stop_sentinel(value: &str) -> bool {
    value == STOP_SENTINEL
}

fn send_command(command: String) -> bool {
    let guard = command_sender()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    guard
        .as_ref()
        .map(|sender| sender.send(command).is_ok())
        .unwrap_or(false)
}

fn clear_bridge_state() {
    *command_sender()
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = None;
    *command_receiver()
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = None;
    STOP_REQUESTED.store(false, Ordering::Relaxed);
    COMMAND_LOOP_READY.store(false, Ordering::Relaxed);
}


#[no_mangle]
pub extern "system" fn Java_com_example_veilknit_1deamon_NativeDaemonBridge_nativeStart(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    context: JObject<'_>,
    data_directory: JString<'_>,
    signup: jboolean,
    username: JString<'_>,
    password: JString<'_>,
) -> jboolean {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return JNI_FALSE;
    }

    let arguments = unowned_env
        .with_env(|env| -> jni::errors::Result<Option<(String, String, String)>> {
            Ok(Some((
                data_directory.try_to_string(env)?,
                username.try_to_string(env)?,
                password.try_to_string(env)?,
            )))
        })
        .resolve::<LogErrorAndDefault>();

    let (data_directory, username, password) = match arguments {
        Some(values) => values,
        None => {
            publish_log("[android] Could not read arguments passed from Kotlin.");
            RUNNING.store(false, Ordering::SeqCst);
            return JNI_FALSE;
        }
    };

    veilid_core::veilid_core_setup_android(unowned_env, context);

    let signup = signup == JNI_TRUE;

    STOP_REQUESTED.store(false, Ordering::SeqCst);
    COMMAND_LOOP_READY.store(false, Ordering::SeqCst);
    log_queue()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clear();

    let (sender, receiver) = mpsc::channel::<String>();
    *command_sender()
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = Some(sender.clone());
    *command_receiver()
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = Some(receiver);

    // Seed the existing login/signup prompt sequence before the runtime starts.
    if sender
        .send(if signup { "s" } else { "l" }.to_string())
        .and_then(|_| sender.send(username))
        .and_then(|_| sender.send(password))
        .is_err()
    {
        clear_bridge_state();
        RUNNING.store(false, Ordering::SeqCst);
        return JNI_FALSE;
    }

    thread::Builder::new()
        .name("veilknit-daemon".to_string())
        .spawn(move || {
            let data_root = PathBuf::from(data_directory);
            if let Err(error) = fs::create_dir_all(&data_root) {
                publish_log(&format!(
                    "[android] Could not create daemon data directory: {error}"
                ));
                clear_bridge_state();
                RUNNING.store(false, Ordering::SeqCst);
                return;
            }
            if let Err(error) = std::env::set_current_dir(&data_root) {
                publish_log(&format!(
                    "[android] Could not select daemon data directory: {error}"
                ));
                clear_bridge_state();
                RUNNING.store(false, Ordering::SeqCst);
                return;
            }
            std::env::set_var("VEILKNIT_DATA_DIR", &data_root);

            let run_result = panic::catch_unwind(AssertUnwindSafe(|| {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .thread_name("veilknit-tokio")
                    .build()
                    .map_err(|error| error.to_string())?;
                runtime
                    .block_on(crate::run_daemon(true))
                    .map_err(|error| error.to_string())
            }));

            match run_result {
                Ok(Ok(())) => publish_log("[android] Daemon stopped."),
                Ok(Err(error)) => publish_log(&format!("[android] Daemon error: {error}")),
                Err(_) => publish_log("[android] Daemon panicked."),
            }

            clear_bridge_state();
            RUNNING.store(false, Ordering::SeqCst);
        })
        .map(|_| JNI_TRUE)
        .unwrap_or_else(|error| {
            publish_log(&format!("[android] Could not start daemon thread: {error}"));
            clear_bridge_state();
            RUNNING.store(false, Ordering::SeqCst);
            JNI_FALSE
        })
}

#[no_mangle]
pub extern "system" fn Java_com_example_veilknit_1deamon_NativeDaemonBridge_nativeSendCommand(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    command: JString<'_>,
) -> jboolean {
    let command = unowned_env
        .with_env(|env| -> jni::errors::Result<Option<String>> {
            Ok(Some(command.try_to_string(env)?))
        })
        .resolve::<LogErrorAndDefault>();

    let Some(command) = command else {
        return JNI_FALSE;
    };

    if send_command(command) {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

#[no_mangle]
pub extern "system" fn Java_com_example_veilknit_1deamon_NativeDaemonBridge_nativeRequestStop(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
) -> jboolean {
    if COMMAND_LOOP_READY.load(Ordering::SeqCst) {
        if send_command("Q".to_string()) {
            JNI_TRUE
        } else {
            JNI_FALSE
        }
    } else {
        STOP_REQUESTED.store(true, Ordering::SeqCst);
        // Unblock a pending login prompt or the first command read.
        let _ = send_command(STOP_SENTINEL.to_string());
        JNI_TRUE
    }
}

#[no_mangle]
pub extern "system" fn Java_com_example_veilknit_1deamon_NativeDaemonBridge_nativeIsRunning(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
) -> jboolean {
    if RUNNING.load(Ordering::SeqCst) {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}


#[no_mangle]
pub extern "system" fn Java_com_example_veilknit_1deamon_NativeDaemonBridge_nativeRestoreBackup(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    data_directory: JString<'_>,
    backup_path: JString<'_>,
    passphrase: JString<'_>,
) -> jstring {
    let arguments = unowned_env
        .with_env(|env| -> jni::errors::Result<Option<(String, String, String)>> {
            Ok(Some((
                data_directory.try_to_string(env)?,
                backup_path.try_to_string(env)?,
                passphrase.try_to_string(env)?,
            )))
        })
        .resolve::<LogErrorAndDefault>();

    let message = match arguments {
        None => "Backup restore failed: Android could not read the selected file details.".to_string(),
        Some(_) if RUNNING.load(Ordering::SeqCst) =>
            "Backup restore failed: stop the daemon before restoring an identity.".to_string(),
        Some((data_directory, backup_path, passphrase)) => {
            let users_root = PathBuf::from(data_directory).join("user_data");
            match crate::user_auth::UserAuth::new(users_root)
                .and_then(|auth| auth.restore_local_backup(&backup_path, &passphrase))
            {
                Ok(metadata) => format!(
                    "Restored account '{}'. Log in with its original account password.",
                    metadata.username
                ),
                Err(error) => format!("Backup restore failed: {error}"),
            }
        }
    };

    unowned_env
        .with_env(|env| -> jni::errors::Result<jstring> {
            let value = JString::from_str(env, message)?;
            Ok(value.into_raw())
        })
        .resolve::<LogErrorAndDefault>()
}

#[no_mangle]
pub extern "system" fn Java_com_example_veilknit_1deamon_NativeDaemonBridge_nativeDrainLogs(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
) -> jstring {
    let lines: Vec<String> = {
        let mut queue = log_queue()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        queue.drain(..).collect()
    };

    let payload = serde_json::to_string(&lines).unwrap_or_else(|_| "[]".to_string());

    unowned_env
        .with_env(|env| -> jni::errors::Result<jstring> {
            let value = JString::from_str(env, payload)?;
            Ok(value.into_raw())
        })
        .resolve::<LogErrorAndDefault>()
}
