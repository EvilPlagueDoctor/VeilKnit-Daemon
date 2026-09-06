//! Optional human-facing console presentation.
//!
//! The network core emits structured events and can compile without the
//! terminal dashboard. Android uses a lightweight bridge implementation.

pub(crate) mod console_log;

#[cfg(target_os = "android")]
#[path = "console_ui_android.rs"]
pub(crate) mod console_ui;

#[cfg(all(not(target_os = "android"), feature = "console-ui"))]
#[path = "console_ui.rs"]
pub(crate) mod console_ui;

#[cfg(all(not(target_os = "android"), not(feature = "console-ui")))]
#[path = "console_ui_stub.rs"]
pub(crate) mod console_ui;
