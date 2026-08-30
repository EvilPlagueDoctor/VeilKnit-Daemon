//! The shutdown record.
//!
//! Everything `report()` prints goes to a log that may not survive: the process can be killed
//! by the host, hard-exited by the watchdog, or force-stopped by the user, and on a phone
//! nobody is reading stdout anyway. This writes the same information somewhere the *next*
//! start can find it.
//!
//! That turns an invisible failure into something a person can hand you from a device you
//! will never touch. It is also what lets a module know a self-check is warranted, instead of
//! paying for one on every launch.
//!
//! # Written twice, on purpose
//!
//! Once by the coordinator when it finishes normally, and once by the watchdog thread just
//! before it forces an exit. The watchdog copy is the important one - a shutdown that
//! completed cleanly is the case nobody needs to investigate.
//!
//! # Read and deleted at startup
//!
//! A record still present when the daemon starts means the previous shutdown did not end the
//! way it intended. Startup logs it, hands it to whichever modules asked, and removes it.

use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use super::{HookOutcome, HookResult, StopReason};

const RECORD_FILE: &str = "last-shutdown.json";
const STAGING_FILE: &str = "last-shutdown.json.tmp";

/// One hook's fate, as it will be read on the next start.
pub struct RecordedHook {
    pub module: String,
    pub tier: String,
    pub detail: String,
    pub outcome: String,
    pub elapsed_ms: u128,
    pub budget_ms: u128,
    pub needs_verification: bool,
}

impl RecordedHook {
    fn from(outcome: &HookOutcome) -> Self {
        Self {
            module: outcome.module.clone(),
            tier: outcome.tier.label().to_string(),
            detail: outcome.detail.clone(),
            outcome: match &outcome.result {
                HookResult::Ok => "ok".into(),
                HookResult::Failed(error) => format!("failed: {error}"),
                HookResult::Overran => "overran".into(),
                HookResult::Skipped(reason) => format!("skipped: {reason}"),
            },
            elapsed_ms: outcome.elapsed.as_millis(),
            budget_ms: outcome.budget.as_millis(),
            needs_verification: outcome.needs_verification,
        }
    }

    fn to_json(&self) -> String {
        format!(
            r#"{{"module":{},"tier":{},"detail":{},"outcome":{},"elapsed_ms":{},"budget_ms":{},"needs_verification":{}}}"#,
            quote(&self.module),
            quote(&self.tier),
            quote(&self.detail),
            quote(&self.outcome),
            self.elapsed_ms,
            self.budget_ms,
            self.needs_verification,
        )
    }
}

/// What the previous shutdown did.
pub struct ShutdownRecord {
    pub written_at: u64,
    pub reason: String,
    /// True when the watchdog wrote this, meaning the process was about to be forced out.
    pub forced: bool,
    pub total_ms: u128,
    pub hooks: Vec<RecordedHook>,
}

impl ShutdownRecord {
    pub fn build(
        reason: StopReason,
        forced: bool,
        total: Duration,
        outcomes: &[HookOutcome],
    ) -> Self {
        Self {
            written_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            reason: format!("{reason:?}"),
            forced,
            total_ms: total.as_millis(),
            hooks: outcomes.iter().map(RecordedHook::from).collect(),
        }
    }

    /// True when nothing needs looking at. A clean record is not worth keeping.
    pub fn is_clean(&self) -> bool {
        !self.forced
            && self
                .hooks
                .iter()
                .all(|hook| hook.outcome == "ok" || hook.outcome.starts_with("skipped"))
    }

    /// Modules that said they were interrupted somewhere unsafe.
    pub fn needing_verification(&self) -> Vec<&RecordedHook> {
        self.hooks
            .iter()
            .filter(|hook| hook.needs_verification)
            .collect()
    }

    fn to_json(&self) -> String {
        let hooks: Vec<String> = self.hooks.iter().map(RecordedHook::to_json).collect();
        format!(
            r#"{{"version":1,"written_at":{},"reason":{},"forced":{},"total_ms":{},"hooks":[{}]}}"#,
            self.written_at,
            quote(&self.reason),
            self.forced,
            self.total_ms,
            hooks.join(",")
        )
    }

    /// Writes the record, replacing any previous one.
    ///
    /// Staged and renamed rather than written in place. This runs at the point where the
    /// process is least likely to survive the next instant, and a truncated record that
    /// cannot be parsed is worse than none: it would look like corruption rather than like the
    /// shutdown it is describing.
    pub fn write(&self, data_dir: &Path) {
        let staging = data_dir.join(STAGING_FILE);
        let target = data_dir.join(RECORD_FILE);
        let outcome = fs::write(&staging, self.to_json())
            .and_then(|()| fs::rename(&staging, &target));
        if let Err(error) = outcome {
            // Only worth a line. Failing to write the post-mortem must never become the
            // reason a shutdown does not complete.
            crate::teprintln!("[lifecycle] could not write the shutdown record: {error}");
        }
    }

    /// Reads and removes any record left by the previous run.
    ///
    /// Removed on read so a single dirty shutdown is not reported at every launch until the
    /// next one happens to be dirty too.
    pub fn take(data_dir: &Path) -> Option<RawRecord> {
        let path = data_dir.join(RECORD_FILE);
        let raw = fs::read_to_string(&path).ok()?;
        let _ = fs::remove_file(&path);
        Some(RawRecord { path, json: raw })
    }
}

/// The previous record as it was found on disk.
///
/// Kept as text rather than parsed here: the daemon already has serde_json, and the value of
/// this file is mostly that a person can read it and send it to you.
pub struct RawRecord {
    pub path: PathBuf,
    pub json: String,
}

impl RawRecord {
    /// True when the record shows a shutdown that did not finish the way it meant to.
    ///
    /// Deliberately a substring check rather than a parse. This runs during startup and must
    /// never be the thing that stops a daemon booting, so it does the cheapest possible test
    /// and leaves interpretation to whoever reads the file.
    pub fn looks_dirty(&self) -> bool {
        self.json.contains(r#""forced":true"#)
            || self.json.contains(r#""outcome":"overran""#)
            || self.json.contains(r#""needs_verification":true"#)
    }
}

fn quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lifecycle::Tier;

    fn outcome(module: &str, result: HookResult, needs_verification: bool) -> HookOutcome {
        HookOutcome {
            module: module.into(),
            detail: "writing pages 3/7".into(),
            tier: Tier::Persist,
            elapsed: Duration::from_millis(4000),
            budget: Duration::from_millis(120),
            result,
            needs_verification,
        }
    }

    #[test]
    fn clean_records_are_recognised() {
        let record = ShutdownRecord::build(
            StopReason::Shutdown,
            false,
            Duration::from_millis(120),
            &[outcome("mailbox", HookResult::Ok, false)],
        );
        assert!(record.is_clean());
    }

    #[test]
    fn overrun_makes_a_record_dirty() {
        let record = ShutdownRecord::build(
            StopReason::Shutdown,
            false,
            Duration::from_millis(4000),
            &[outcome("mailbox", HookResult::Overran, false)],
        );
        assert!(!record.is_clean());
        assert!(RawRecord {
            path: PathBuf::new(),
            json: record.to_json()
        }
        .looks_dirty());
    }

    #[test]
    fn verification_flag_survives_the_round_trip() {
        let record = ShutdownRecord::build(
            StopReason::Shutdown,
            true,
            Duration::from_millis(40000),
            &[outcome("mailbox", HookResult::Overran, true)],
        );
        assert_eq!(record.needing_verification().len(), 1);
        assert!(record.to_json().contains(r#""needs_verification":true"#));
        assert!(record.to_json().contains("writing pages 3/7"));
    }

    #[test]
    fn detail_text_is_escaped() {
        let mut hook = outcome("mailbox", HookResult::Overran, false);
        hook.detail = "writing \"pages\"\n3/7".into();
        let record =
            ShutdownRecord::build(StopReason::Shutdown, false, Duration::from_millis(1), &[hook]);
        let json = record.to_json();
        assert!(json.contains(r#"\"pages\""#));
        assert!(!json.contains('\n'));
    }
}
