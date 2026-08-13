//! Optional human-facing console presentation.
//!
//! The network core emits structured events and can compile without the
//! terminal dashboard. With the `console-ui` feature disabled, a small stub
//! keeps the core API available without pulling in terminal dependencies.

pub(crate) mod console_log;

#[cfg(feature = "console-ui")]
#[path = "console_ui.rs"]
pub(crate) mod console_ui;

#[cfg(not(feature = "console-ui"))]
#[path = "console_ui_stub.rs"]
pub(crate) mod console_ui;
