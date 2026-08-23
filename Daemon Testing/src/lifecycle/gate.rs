//! Startup readiness gating.
//!
//! `StartupStage` already existed as telemetry - `stage_running()` marked a stage started and
//! `complete()` recorded its elapsed time - but nothing ever waited on one. Ordering was
//! therefore the literal sequence of statements in `lib.rs`, with real dependencies recorded
//! in comments. This turns the existing reporting into gating.
//!
//! A `watch` channel per stage rather than a polling loop: awaiting one yields the worker
//! exactly as `tokio::time::sleep` does, but wakes the instant the stage completes instead of
//! up to a poll interval later, costs nothing while idle, and lets several modules await the
//! same stage without coordinating.

use std::{collections::HashMap, sync::Arc, time::Duration};

use tokio::sync::watch;

use crate::events::network_events::StartupStage;

/// Every stage a module might wait on.
const ALL_STAGES: [StartupStage; 16] = [
    StartupStage::Configuration,
    StartupStage::Identity,
    StartupStage::Reputation,
    StartupStage::Veilid,
    StartupStage::NetworkAttachment,
    StartupStage::DhtRestore,
    StartupStage::MainDht,
    StartupStage::DhtNetworkVerification,
    StartupStage::Presence,
    StartupStage::Routes,
    StartupStage::Handshake,
    StartupStage::Mailbox,
    StartupStage::Walker,
    StartupStage::ApplicationInfo,
    StartupStage::BackgroundServices,
    StartupStage::Ready,
];

/// How a stage finished, from a waiter's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageOutcome {
    Pending,
    Ready,
    /// The stage failed or was skipped. Waiters are released rather than left hanging, and
    /// decide for themselves whether they can proceed without it.
    Unavailable,
}

/// One stage's readiness signal.
#[derive(Clone)]
pub struct StageGate {
    stage: StartupStage,
    rx: watch::Receiver<StageOutcome>,
}

impl StageGate {
    /// Waits until the stage resolves. Returns immediately if it already has.
    pub async fn wait(&self) -> StageOutcome {
        let mut rx = self.rx.clone();
        loop {
            let current = *rx.borrow();
            if current != StageOutcome::Pending {
                return current;
            }
            if rx.changed().await.is_err() {
                // Sender dropped, which means startup is being torn down. Release the waiter
                // rather than parking it forever.
                return StageOutcome::Unavailable;
            }
        }
    }

    /// Waits with a ceiling. A module that can degrade rather than block should prefer this.
    pub async fn wait_for(&self, limit: Duration) -> StageOutcome {
        match tokio::time::timeout(limit, self.wait()).await {
            Ok(outcome) => outcome,
            Err(_) => {
                crate::teprintln!(
                    "[lifecycle] waited {}s for {:?} without it becoming ready",
                    limit.as_secs(),
                    self.stage
                );
                StageOutcome::Pending
            }
        }
    }

    /// Non-blocking check.
    pub fn peek(&self) -> StageOutcome {
        *self.rx.borrow()
    }

    pub fn stage(&self) -> StartupStage {
        self.stage
    }
}

/// The full set of stage gates.
#[derive(Clone)]
pub struct StageGates {
    senders: Arc<HashMap<StartupStage, watch::Sender<StageOutcome>>>,
    receivers: Arc<HashMap<StartupStage, watch::Receiver<StageOutcome>>>,
}

impl Default for StageGates {
    fn default() -> Self {
        Self::new()
    }
}

impl StageGates {
    pub fn new() -> Self {
        let mut senders = HashMap::new();
        let mut receivers = HashMap::new();
        for stage in ALL_STAGES {
            let (tx, rx) = watch::channel(StageOutcome::Pending);
            senders.insert(stage, tx);
            receivers.insert(stage, rx);
        }
        Self {
            senders: Arc::new(senders),
            receivers: Arc::new(receivers),
        }
    }

    /// A handle a module can await. Declare these where the module is constructed, so its
    /// dependencies are visible at its own definition rather than in `lib.rs`.
    pub fn gate(&self, stage: StartupStage) -> StageGate {
        let rx = self
            .receivers
            .get(&stage)
            .expect("every StartupStage variant is registered in ALL_STAGES")
            .clone();
        StageGate { stage, rx }
    }

    /// Marks a stage ready and wakes everything waiting on it.
    pub fn mark_ready(&self, stage: StartupStage) {
        self.set(stage, StageOutcome::Ready);
    }

    /// Marks a stage as failed or skipped. Waiters are released with `Unavailable` so they can
    /// decide whether to degrade or give up, rather than being parked forever.
    pub fn mark_unavailable(&self, stage: StartupStage) {
        self.set(stage, StageOutcome::Unavailable);
    }

    fn set(&self, stage: StartupStage, outcome: StageOutcome) {
        if let Some(tx) = self.senders.get(&stage) {
            let _ = tx.send(outcome);
        }
    }

    /// Convenience for waiting on several stages at once. Returns the worst outcome, so a
    /// caller needing everything can check one value.
    pub async fn wait_all(&self, stages: &[StartupStage]) -> StageOutcome {
        let mut worst = StageOutcome::Ready;
        for stage in stages {
            match self.gate(*stage).wait().await {
                StageOutcome::Ready => {}
                StageOutcome::Unavailable => worst = StageOutcome::Unavailable,
                StageOutcome::Pending => {
                    if worst == StageOutcome::Ready {
                        worst = StageOutcome::Pending;
                    }
                }
            }
        }
        worst
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn waiter_wakes_when_the_stage_completes() {
        let gates = StageGates::new();
        let gate = gates.gate(StartupStage::MainDht);

        let waiter = tokio::spawn(async move { gate.wait().await });
        tokio::task::yield_now().await;
        gates.mark_ready(StartupStage::MainDht);

        assert_eq!(waiter.await.unwrap(), StageOutcome::Ready);
    }

    #[tokio::test]
    async fn already_ready_returns_without_waiting() {
        let gates = StageGates::new();
        gates.mark_ready(StartupStage::Veilid);
        assert_eq!(gates.gate(StartupStage::Veilid).wait().await, StageOutcome::Ready);
    }

    #[tokio::test]
    async fn failed_stage_releases_waiters() {
        let gates = StageGates::new();
        let gate = gates.gate(StartupStage::Routes);

        let waiter = tokio::spawn(async move { gate.wait().await });
        tokio::task::yield_now().await;
        gates.mark_unavailable(StartupStage::Routes);

        assert_eq!(waiter.await.unwrap(), StageOutcome::Unavailable);
    }

    #[tokio::test]
    async fn wait_for_gives_up_rather_than_hanging() {
        let gates = StageGates::new();
        let outcome = gates
            .gate(StartupStage::Mailbox)
            .wait_for(Duration::from_millis(50))
            .await;
        assert_eq!(outcome, StageOutcome::Pending);
    }
}
