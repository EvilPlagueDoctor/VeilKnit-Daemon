//! Desktop stand-in for `android_bridge`.
//!
//! The Android build reaches the host through JNI: log lines go to a buffer Kotlin polls,
//! stop requests arrive from the foreground service, and network changes are reported by the
//! Android connectivity callbacks. None of that exists on a desktop, but `node` and
//! `user_dht` call into it regardless, so this provides the same functions with the same
//! signatures and desktop-appropriate behaviour.
//!
//! Deliberately the same module shape rather than a set of `#[cfg]` blocks scattered through
//! the callers: the daemon's own rule is that one module owns one concern, and "how we talk
//! to the host" is a concern. Swapping the implementation should not mean editing `node`.
//!
//! # The network generation is real here, and better than Android's
//!
//! `android_bridge::network_change_generation()` returns a hardcoded `0`, so the re-attach
//! logic in `node::wait_for_network` that watches it can never fire. That is part of why a
//! node which went offline after startup stayed offline.
//!
//! Here the counter is genuine and can be bumped by hand from the console. Testing the
//! re-attach path on a phone means physically toggling wifi and hoping; on the bench it is a
//! menu command, which makes the twelve-hour-outage case something that can be reproduced on
//! demand rather than waited for.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Set by the Ctrl-C handler, and by the console's quit command.
static STOP_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Bumped whenever the host reports the network changed. On Android this would come from a
/// connectivity callback; here it is manual, which is the point.
static NETWORK_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Free-form description of the active network, for logs.
static NETWORK_DESCRIPTION: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

// ---------------------------------------------------------------------------
// The four functions `node` and `user_dht` call
// ---------------------------------------------------------------------------

pub(crate) fn stop_requested() -> bool {
    STOP_REQUESTED.load(Ordering::SeqCst)
}

pub(crate) fn network_change_generation() -> u64 {
    NETWORK_GENERATION.load(Ordering::Relaxed)
}

pub(crate) fn network_description() -> &'static str {
    // The callers take `&'static str`, so a leak is the honest way to return a value that can
    // change at runtime. It happens at most once per simulated network change, which over a
    // bench session is a handful of short strings.
    match NETWORK_DESCRIPTION.lock() {
        Ok(guard) => match guard.as_ref() {
            Some(description) => Box::leak(description.clone().into_boxed_str()),
            None => "desktop network",
        },
        Err(_) => "desktop network (description unavailable)",
    }
}

/// Present for signature parity with the Android bridge. The desktop console prints directly,
/// so there is no buffer to publish into.
#[allow(dead_code)]
pub(crate) fn publish_log(_line: &str) {}

// ---------------------------------------------------------------------------
// Driving it from the bench
// ---------------------------------------------------------------------------

/// Requests a graceful stop. Wired to Ctrl-C and to the console's quit command.
pub(crate) fn request_stop() {
    STOP_REQUESTED.store(true, Ordering::SeqCst);
}

/// Clears the stop flag. Needed for restart, which runs the stop sequence and then starts
/// again in the same process - without this the fresh run would see a stop already pending.
pub(crate) fn clear_stop() {
    STOP_REQUESTED.store(false, Ordering::SeqCst);
}

/// Simulates the host reporting a network change.
///
/// This is what makes the re-attach path testable. `node::wait_for_network` watches this
/// counter; bumping it should make the node notice, and if it does not, that is the bug we
/// went looking for after the twelve-hour outage.
pub(crate) fn simulate_network_change(description: &str) -> u64 {
    if let Ok(mut guard) = NETWORK_DESCRIPTION.lock() {
        *guard = Some(description.to_string());
    }
    let generation = NETWORK_GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    crate::tprintln!(
        "[bridge] simulated network change -> generation {generation} ({description})"
    );
    generation
}

/// Installs the Ctrl-C handler. Call once at startup.
///
/// A second Ctrl-C exits immediately: if the first one is being swallowed by a stuck shutdown
/// hook, the operator should not have to reach for the task manager to find out.
pub(crate) fn install_signal_handler() {
    let already_asked = std::sync::Arc::new(AtomicBool::new(false));
    let result = ctrlc::set_handler(move || {
        if already_asked.swap(true, Ordering::SeqCst) {
            eprintln!("[bridge] second interrupt received; exiting immediately");
            std::process::exit(130);
        }
        eprintln!("[bridge] interrupt received; requesting graceful stop (again to force)");
        request_stop();
    });
    if let Err(error) = result {
        crate::teprintln!("[bridge] could not install the interrupt handler: {error}");
    }
}
