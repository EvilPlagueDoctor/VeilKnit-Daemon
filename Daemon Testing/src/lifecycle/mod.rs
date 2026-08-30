//! Lifecycle coordination: startup readiness gating and tiered shutdown.
//!
//! This module owns *sequencing*, never actions. A module registers what it needs and how
//! long it should get; it performs its own work in its own hook and handles its own
//! crash-safety. The coordinator never reaches into another module's state, because doing so
//! would reintroduce the multiple-ownership problem it exists to remove.
//!
//! # Ordering is declared, not maintained
//!
//! There is one axis: the resources a hook needs available while it runs. Order falls out of
//! that, so adding a module never means editing this file.
//!
//! | Need | Runs | When unreachable |
//! |---|---|---|
//! | [`ResourceNeed::Network`] | before Veilid detaches | skipped as a tier |
//! | [`ResourceNeed::VeilidNode`] | before Veilid stops | still runs |
//! | [`ResourceNeed::Storage`] | any point, including after teardown | still runs |
//! | [`ResourceNeed::None`] | any point | still runs |
//!
//! # Why tiers run concurrently
//!
//! Single ownership means two hooks in the same tier cannot touch each other's state, so they
//! are safe to run at once. Wall time for a tier is its slowest hook rather than the sum,
//! which is what makes generous per-hook budgets affordable.
//!
//! # Why the watchdog is a thread
//!
//! A hook that does blocking work occupies a runtime worker. Enough of those and a timer task
//! never gets polled, so a deadline expressed as a task can silently fail to fire - the same
//! defect shape as holding a mutex across a blocking receive. The watchdog therefore runs on
//! its own OS thread and can hard-exit regardless of what the runtime is doing.
//!
//! That is safe because the network already tolerates a peer vanishing without notice; the
//! host OS kills this process on its own schedule regardless. A guaranteed exit beats a
//! graceful one that might never arrive.

use std::{
    collections::BTreeMap,
    fmt,
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::{sync::Mutex, task::JoinSet, time::timeout};

use crate::events::network_events::StartupStage;

mod gate;
mod record;

pub use gate::{StageGate, StageGates, StageOutcome};
pub use record::{RawRecord, RecordedHook, ShutdownRecord};

// ---------------------------------------------------------------------------
// Budgets
// ---------------------------------------------------------------------------

/// Per-tier ceilings. Sized for older hardware: the concern stretches the tiers themselves,
/// not only the total, so scaling the watchdog alone would just mean tiers timing out
/// individually inside a longer window.
pub mod budget {
    use std::time::Duration;

    pub const INTAKE: Duration = Duration::from_secs(4);
    pub const ANNOUNCE: Duration = Duration::from_secs(6);
    pub const NODE: Duration = Duration::from_secs(4);
    pub const PERSIST: Duration = Duration::from_secs(6);
    pub const TEARDOWN: Duration = Duration::from_secs(10);
    pub const AFTER: Duration = Duration::from_secs(2);

    /// Hard exit if graceful shutdown overruns this. Comfortably above the sum of the tiers,
    /// which is roughly 32 seconds.
    pub const WATCHDOG: Duration = Duration::from_secs(40);

    /// Default per-hook ceiling when a module does not state one.
    pub const DEFAULT_HOOK: Duration = Duration::from_secs(3);
}

// ---------------------------------------------------------------------------
// Declarations
// ---------------------------------------------------------------------------

/// What a hook needs available while it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ResourceNeed {
    /// Nothing. In-memory teardown only.
    None,
    /// Local disk. Runs even after Veilid is gone.
    Storage,
    /// The Veilid node object alive, but not necessarily reachable.
    VeilidNode,
    /// Genuine reachability. Skipped as a tier when the node cannot reach anyone.
    Network,
}

/// Where in the sequence a need places a hook. Derived, never declared directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// Stop accepting new work. Runs first so everything after it faces a quiet system.
    Intake,
    /// Tell the network we are going away. Requires reachability.
    Announce,
    /// Needs the node object but not the network.
    NodeDependent,
    /// Local persistence.
    Persist,
    /// Veilid detach and shutdown.
    Teardown,
    /// Anything that must outlive Veilid. Log flushing today.
    After,
}

impl Tier {
    pub fn label(self) -> &'static str {
        match self {
            Self::Intake => "intake stop",
            Self::Announce => "announce",
            Self::NodeDependent => "node",
            Self::Persist => "persist",
            Self::Teardown => "teardown",
            Self::After => "after",
        }
    }

    /// Tiers in execution order.
    pub const ORDER: [Tier; 6] = [
        Tier::Intake,
        Tier::Announce,
        Tier::NodeDependent,
        Tier::Persist,
        Tier::Teardown,
        Tier::After,
    ];
}

/// Why the coordinator is running. Restart differs from shutdown in exactly one way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    Shutdown,
    /// Coming back in seconds. The announce tier is skipped entirely: a logout immediately
    /// followed by a login is presence churn that tells peers nothing, and the presence
    /// heartbeat interval leaves far more margin than a restart consumes.
    Restart,
}

/// What the coordinator knows about reachability when it starts.
///
/// Supplied by whichever module owns attachment. `Unknown` is resolved by a cheap read before
/// the announce tier: reading a timestamp costs far less than attempting the tier and burning
/// a budget per hook discovering the network is dead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reachability {
    Reachable,
    Unreachable,
    Unknown,
}

// ---------------------------------------------------------------------------
// Hooks
// ---------------------------------------------------------------------------

type HookFuture = std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send>>;
type HookAction = Box<dyn Fn(Progress) -> HookFuture + Send + Sync>;

/// A hook's running commentary on what it is doing.
///
/// The single highest-value thing a shutdown log can carry. "mailbox overran" names a module
/// with six thousand lines in it; "mailbox overran while writing pages 3/7" names the line.
/// Because it is read after the hook has been abandoned, it has to live outside the hook's
/// future - a cancelled future takes its locals with it.
#[derive(Clone)]
pub struct Progress {
    step: Arc<std::sync::Mutex<String>>,
    /// Set by a hook that knows being interrupted here leaves state needing a check on the
    /// next start. The coordinator never infers this: only the module knows whether its own
    /// half-finished write is survivable.
    needs_verification: Arc<std::sync::atomic::AtomicBool>,
}

impl Progress {
    fn new(initial: &str) -> Self {
        Self {
            step: Arc::new(std::sync::Mutex::new(initial.to_string())),
            needs_verification: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Records what the hook is doing now. Call it before each phase, not after.
    pub fn step(&self, description: impl Into<String>) {
        if let Ok(mut guard) = self.step.lock() {
            *guard = description.into();
        }
    }

    /// Marks the current step as one that leaves state needing verification if interrupted.
    /// Pair with [`Progress::step`] around anything that rewrites a file in place.
    pub fn fragile(&self, description: impl Into<String>) {
        self.needs_verification
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.step(description);
    }

    /// Clears the fragile flag once the risky part is safely behind us.
    pub fn settled(&self, description: impl Into<String>) {
        self.needs_verification
            .store(false, std::sync::atomic::Ordering::Relaxed);
        self.step(description);
    }

    fn read(&self) -> String {
        self.step
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_else(|_| "unknown".to_string())
    }

    fn verification_needed(&self) -> bool {
        self.needs_verification
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}

struct Hook {
    module: String,
    detail: String,
    need: ResourceNeed,
    budget: Duration,
    /// Hooks a restart may skip. Withdrawal of open service requests is the case this exists
    /// for: skipping it costs only uninitiated handshakes against a host that has gone.
    optional: bool,
    action: HookAction,
}

impl Hook {
    fn tier(&self, is_teardown: bool) -> Tier {
        if is_teardown {
            return Tier::Teardown;
        }
        match self.need {
            ResourceNeed::Network => Tier::Announce,
            ResourceNeed::VeilidNode => Tier::NodeDependent,
            ResourceNeed::Storage => Tier::Persist,
            ResourceNeed::None => Tier::Intake,
        }
    }
}

/// How a hook finished. Recorded for every hook, whether or not timings are being collected.
#[derive(Debug, Clone)]
pub struct HookOutcome {
    pub module: String,
    /// What the hook was doing when it finished or was abandoned.
    pub detail: String,
    pub tier: Tier,
    pub elapsed: Duration,
    pub budget: Duration,
    pub result: HookResult,
    /// The hook was interrupted somewhere it considers unsafe, and said so itself.
    pub needs_verification: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookResult {
    Ok,
    Failed(String),
    /// Exceeded its budget and was dropped mid-flight.
    Overran,
    /// Not attempted. Carries the reason so the log explains itself.
    Skipped(&'static str),
}

impl fmt::Display for HookOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status = match &self.result {
            HookResult::Ok => "ok".to_string(),
            HookResult::Failed(error) => format!("failed: {error}"),
            HookResult::Overran => "OVERRAN".to_string(),
            HookResult::Skipped(reason) => format!("skipped ({reason})"),
        };
        let detail = if self.detail.is_empty() {
            String::new()
        } else {
            format!("  [{}]", self.detail)
        };
        let flag = if self.needs_verification {
            "  NEEDS VERIFICATION"
        } else {
            ""
        };
        write!(
            f,
            "{:<10} {:<26} {:>6}ms / {:>5}ms  {}{}{}",
            self.tier.label(),
            self.module,
            self.elapsed.as_millis(),
            self.budget.as_millis(),
            status,
            detail,
            flag
        )
    }
}

// ---------------------------------------------------------------------------
// Coordinator
// ---------------------------------------------------------------------------

/// Owns startup gating and shutdown sequencing.
///
/// Cloneable; all clones share one registry, so a module can register from wherever it is
/// constructed without threading a reference back to `lib.rs`.
#[derive(Clone)]
pub struct Lifecycle {
    hooks: Arc<Mutex<Vec<Hook>>>,
    gates: StageGates,
    stopping: Arc<std::sync::atomic::AtomicBool>,
    collect_timings: Arc<std::sync::atomic::AtomicBool>,
    budgets: Budgets,
    watchdog_action: WatchdogAction,
    watchdog_fired: Arc<std::sync::atomic::AtomicBool>,
    /// Where the shutdown record is written. None disables it, which is what the bench wants.
    data_dir: Arc<std::sync::Mutex<Option<std::path::PathBuf>>>,
    /// Shared with the watchdog thread so a forced exit can still write what it knew.
    progress_so_far: Arc<std::sync::Mutex<Vec<HookOutcome>>>,
}

impl Default for Lifecycle {
    fn default() -> Self {
        Self::new()
    }
}

impl Lifecycle {
    pub fn new() -> Self {
        Self::with(Budgets::default(), WatchdogAction::Exit)
    }

    pub fn with(budgets: Budgets, watchdog_action: WatchdogAction) -> Self {
        Self {
            hooks: Arc::new(Mutex::new(Vec::new())),
            gates: StageGates::new(),
            stopping: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            collect_timings: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            budgets,
            watchdog_action,
            watchdog_fired: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            data_dir: Arc::new(std::sync::Mutex::new(None)),
            progress_so_far: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    /// Enables the shutdown record and says where to keep it.
    pub fn set_data_dir(&self, path: impl Into<std::path::PathBuf>) {
        if let Ok(mut guard) = self.data_dir.lock() {
            *guard = Some(path.into());
        }
    }

    /// Reads and clears any record the previous run left behind.
    ///
    /// Call once at startup, before anything else. A record still present means the last
    /// shutdown did not end the way it intended.
    pub fn take_previous_record(&self) -> Option<RawRecord> {
        let guard = self.data_dir.lock().ok()?;
        let dir = guard.as_ref()?;
        ShutdownRecord::take(dir)
    }

    pub fn budgets(&self) -> Budgets {
        self.budgets
    }

    /// True when the watchdog tripped. Only meaningful under [`WatchdogAction::Report`].
    pub fn watchdog_fired(&self) -> bool {
        self.watchdog_fired
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Startup readiness. Modules await the stages they declared rather than relying on the
    /// order of statements in `lib.rs`.
    pub fn gates(&self) -> &StageGates {
        &self.gates
    }

    /// Enables the opt-in timing report. Off by default; driven by a setting in the GUI.
    pub fn set_collect_timings(&self, enabled: bool) {
        self.collect_timings
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
    }

    /// True once a stop has begun. Long-running loops should check this and bail rather than
    /// starting new work the coordinator will have to wait for.
    pub fn is_stopping(&self) -> bool {
        self.stopping.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Registers a hook.
    ///
    /// The closure receives a [`Progress`] handle. Using it is optional but strongly
    /// encouraged for anything with more than one phase: it is the difference between a log
    /// that names a module and one that names a step.
    pub async fn register<F, Fut>(&self, spec: HookSpec, action: F)
    where
        F: Fn(Progress) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<(), String>> + Send + 'static,
    {
        self.hooks.lock().await.push(Hook {
            module: spec.module,
            detail: spec.detail,
            need: spec.need,
            budget: spec.budget,
            optional: spec.optional,
            action: Box::new(move |progress| Box::pin(action(progress))),
        });
    }

    /// Runs the tiers in order and returns every outcome.
    ///
    /// Idempotent: a second call while a stop is already running returns immediately rather
    /// than executing hooks twice. Pressing shutdown twice is normal user behaviour.
    pub async fn stop(
        &self,
        reason: StopReason,
        reachability: Reachability,
        refresh_reachability: impl std::future::Future<Output = Reachability> + Send,
    ) -> Vec<HookOutcome> {
        if self
            .stopping
            .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            crate::tprintln!("[lifecycle] stop already in progress; ignoring duplicate request");
            return Vec::new();
        }

        if let Ok(mut guard) = self.progress_so_far.lock() {
            guard.clear();
        }
        let watchdog = Watchdog::arm(
            self.budgets.watchdog,
            self.watchdog_action,
            self.watchdog_fired.clone(),
            self.data_dir.lock().ok().and_then(|guard| guard.clone()),
            self.progress_so_far.clone(),
            reason,
        );
        let started = Instant::now();
        crate::tprintln!("[lifecycle] stopping ({reason:?})");

        // Resolve Unknown once, before the announce tier, with a cheap read rather than by
        // attempting the tier and discovering it the expensive way.
        let reachable = match reachability {
            Reachability::Reachable => true,
            Reachability::Unreachable => false,
            Reachability::Unknown => {
                matches!(refresh_reachability.await, Reachability::Reachable)
            }
        };

        let mut hooks = std::mem::take(&mut *self.hooks.lock().await);
        let mut by_tier: BTreeMap<Tier, Vec<Hook>> = BTreeMap::new();
        // Veilid teardown is identified by need plus module name rather than by a separate
        // flag: it is the only hook that must run after everything else needing the node.
        for hook in hooks.drain(..) {
            let is_teardown = hook.module == TEARDOWN_MODULE;
            by_tier.entry(hook.tier(is_teardown)).or_default().push(hook);
        }

        let mut outcomes = Vec::new();
        for tier in Tier::ORDER {
            let Some(tier_hooks) = by_tier.remove(&tier) else {
                continue;
            };
            if tier_hooks.is_empty() {
                continue;
            }

            let skip = match tier {
                Tier::Announce if reason == StopReason::Restart => Some("restart"),
                Tier::Announce if !reachable => Some("network unreachable"),
                _ => None,
            };
            if let Some(reason) = skip {
                for hook in tier_hooks {
                    outcomes.push(HookOutcome {
                        module: hook.module,
                        detail: hook.detail,
                        tier,
                        elapsed: Duration::ZERO,
                        budget: hook.budget,
                        result: HookResult::Skipped(reason),
                        needs_verification: false,
                    });
                }
                crate::tprintln!("[lifecycle] {} tier skipped ({reason})", tier.label());
                continue;
            }

            let tier_outcomes = run_tier(tier, tier_hooks, self.budgets).await;
            if let Ok(mut guard) = self.progress_so_far.lock() {
                guard.extend(tier_outcomes.iter().cloned());
            }
            outcomes.extend(tier_outcomes);

            // Production hard-exits when the watchdog fires. Report mode has to model that or
            // the bench sits through work the real daemon would never have reached, and the
            // measured total bears no relation to what would actually happen.
            if self.watchdog_action == WatchdogAction::Report && self.watchdog_fired() {
                crate::teprintln!(
                    "[lifecycle] watchdog fired; abandoning the remaining tiers as a hard exit would"
                );
                break;
            }
        }

        let elapsed = started.elapsed();
        watchdog.disarm();

        report(&outcomes, elapsed);

        // Written whether or not anything went wrong: a clean record is deleted immediately
        // below, and its absence is itself the signal that the last shutdown was fine.
        if let Ok(guard) = self.data_dir.lock() {
            if let Some(dir) = guard.as_ref() {
                let record = ShutdownRecord::build(reason, false, elapsed, &outcomes);
                if record.is_clean() {
                    let _ = std::fs::remove_file(dir.join("last-shutdown.json"));
                } else {
                    record.write(dir);
                }
            }
        }
        if self.collect_timings.load(std::sync::atomic::Ordering::Relaxed) {
            emit_timing_report(&outcomes, elapsed);
        }
        outcomes
    }
}

/// Tier ceilings and watchdog limit, overridable so the bench can exercise overrun paths in
/// milliseconds instead of tens of seconds.
#[derive(Debug, Clone, Copy)]
pub struct Budgets {
    pub intake: Duration,
    pub announce: Duration,
    pub node: Duration,
    pub persist: Duration,
    pub teardown: Duration,
    pub after: Duration,
    pub watchdog: Duration,
    pub default_hook: Duration,
}

impl Default for Budgets {
    fn default() -> Self {
        Self {
            intake: budget::INTAKE,
            announce: budget::ANNOUNCE,
            node: budget::NODE,
            persist: budget::PERSIST,
            teardown: budget::TEARDOWN,
            after: budget::AFTER,
            watchdog: budget::WATCHDOG,
            default_hook: budget::DEFAULT_HOOK,
        }
    }
}

impl Budgets {
    pub fn for_tier(&self, tier: Tier) -> Duration {
        match tier {
            Tier::Intake => self.intake,
            Tier::Announce => self.announce,
            Tier::NodeDependent => self.node,
            Tier::Persist => self.persist,
            Tier::Teardown => self.teardown,
            Tier::After => self.after,
        }
    }

    /// Scales every ceiling. Used by the bench so a watchdog scenario finishes in under a
    /// second rather than forty.
    pub fn scaled(mut self, factor: f64) -> Self {
        let scale = |d: Duration| Duration::from_secs_f64((d.as_secs_f64() * factor).max(0.001));
        self.intake = scale(self.intake);
        self.announce = scale(self.announce);
        self.node = scale(self.node);
        self.persist = scale(self.persist);
        self.teardown = scale(self.teardown);
        self.after = scale(self.after);
        self.watchdog = scale(self.watchdog);
        self.default_hook = scale(self.default_hook);
        self
    }
}

/// What the watchdog does when graceful shutdown overruns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchdogAction {
    /// Production: hard-exit. Nothing downstream depends on a clean exit.
    Exit,
    /// Bench: record that it fired so the scenario can assert on it. A process that exits
    /// cannot report what it observed, which makes the production behaviour untestable.
    Report,
}

/// Module name reserved for the hook that tears Veilid down. Placed in its own tier so
/// everything needing the node has already finished.
pub const TEARDOWN_MODULE: &str = "node/veilid";

/// What a module declares when it registers.
pub struct HookSpec {
    pub module: String,
    pub detail: String,
    pub need: ResourceNeed,
    pub budget: Duration,
    pub optional: bool,
}

impl HookSpec {
    pub fn new(module: impl Into<String>, need: ResourceNeed) -> Self {
        Self {
            module: module.into(),
            detail: String::new(),
            need,
            budget: budget::DEFAULT_HOOK,
            optional: false,
        }
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = detail.into();
        self
    }

    pub fn budget(mut self, budget: Duration) -> Self {
        self.budget = budget;
        self
    }

    /// Marks a hook that may be dropped when the tier runs short.
    pub fn optional(mut self) -> Self {
        self.optional = true;
        self
    }
}

// ---------------------------------------------------------------------------
// Tier execution
// ---------------------------------------------------------------------------

async fn run_tier(tier: Tier, hooks: Vec<Hook>, budgets: Budgets) -> Vec<HookOutcome> {
    let tier_budget = budgets.for_tier(tier);
    let mut set = JoinSet::new();

    // Recorded up front: once a hook is moved into the JoinSet its identity is gone, and the
    // abandonment path below needs to know who never reported.
    // A Progress handle per hook, held by both the hook and the tier. The tier keeps its copy
    // so an abandoned hook's last reported step can still be read after its future is gone.
    let progress: Vec<(String, Progress)> = hooks
        .iter()
        .map(|hook| {
            let handle = Progress::new(if hook.detail.is_empty() { "starting" } else { &hook.detail });
            (hook.module.clone(), handle)
        })
        .collect();

    let expected: Vec<(String, Progress, Duration)> = hooks
        .iter()
        .zip(progress.iter())
        .map(|(hook, (_, handle))| {
            (hook.module.clone(), handle.clone(), hook.budget.min(tier_budget))
        })
        .collect();

    for hook in hooks {
        // Never let a hook's own budget exceed its tier's, or a single module could consume
        // the whole tier and starve its neighbours out of their share.
        let hook_budget = hook.budget.min(tier_budget);
        let module = hook.module.clone();
        let handle = progress
            .iter()
            .find(|(name, _)| name == &module)
            .map(|(_, handle)| handle.clone())
            .unwrap_or_else(|| Progress::new("starting"));
        let reporter = handle.clone();
        let action = hook.action;

        // spawn_blocking rather than spawn: hooks run on the blocking pool, which is separate
        // from the worker threads. A hook that blocks its thread then starves nothing, so the
        // timers supervising it keep being polled and its neighbours keep running.
        //
        // Handle::block_on is legal here because a blocking-pool thread is not an async
        // context. Doing this on a worker thread would panic, which is precisely the
        // distinction that matters.
        let runtime = tokio::runtime::Handle::current();
        set.spawn_blocking(move || {
            let started = Instant::now();
            let outcome = runtime.block_on(async {
                match timeout(hook_budget, action(reporter)).await {
                    Ok(Ok(())) => None,
                    Ok(Err(error)) => Some(HookResult::Failed(error)),
                    Err(_) => Some(HookResult::Overran),
                }
            });
            let elapsed = started.elapsed();

            // A completed future is not the same as a hook that behaved. If it could not be
            // cancelled - because it blocked rather than awaited - `timeout` returns success
            // once it finally finishes, and the budget it blew through goes unrecorded.
            // Judging on measured time closes that hole regardless of why the timeout missed.
            let result = match outcome {
                Some(result) => result,
                None if elapsed > hook_budget => HookResult::Overran,
                None => HookResult::Ok,
            };

            HookOutcome {
                module,
                detail: handle.read(),
                tier,
                elapsed,
                budget: hook_budget,
                result,
                needs_verification: handle.verification_needed(),
            }
        });
    }

    let mut outcomes = Vec::new();
    // The tier ceiling is a second line of defence. Per-hook timeouts should make it
    // unreachable, but a hook that blocks its worker thread cannot be timed out from inside
    // the runtime at all - the timeout future queues behind the very thing it should cancel.
    match timeout(tier_budget, async {
        while let Some(joined) = set.join_next().await {
            match joined {
                Ok(outcome) => outcomes.push(outcome),
                Err(error) => crate::teprintln!("[lifecycle] hook task failed to join: {error}"),
            }
        }
    })
    .await
    {
        Ok(()) => {}
        Err(_) => {
            crate::teprintln!(
                "[lifecycle] {} tier exceeded its {}ms ceiling; abandoning outstanding hooks",
                tier.label(),
                tier_budget.as_millis()
            );
            set.abort_all();

            // Anything that never reported is Overran, recorded here rather than left to
            // report Ok whenever it eventually finishes. A hook that blew its budget many
            // times over must never appear in the summary as a success - that is the failure
            // that makes a green run untrustworthy.
            let reported: std::collections::BTreeSet<String> =
                outcomes.iter().map(|outcome| outcome.module.clone()).collect();
            for (module, handle, budget) in expected {
                if !reported.contains(&module) {
                    outcomes.push(HookOutcome {
                        module,
                        // Read from the handle the tier kept, not from the hook: the hook's
                        // future has been dropped and everything it owned went with it.
                        detail: handle.read(),
                        tier,
                        elapsed: tier_budget,
                        budget,
                        result: HookResult::Overran,
                        needs_verification: handle.verification_needed(),
                    });
                }
            }
        }
    }

    outcomes
}

fn report(outcomes: &[HookOutcome], elapsed: Duration) {
    let overran: Vec<&HookOutcome> = outcomes
        .iter()
        .filter(|outcome| matches!(outcome.result, HookResult::Overran))
        .collect();
    let failed: Vec<&HookOutcome> = outcomes
        .iter()
        .filter(|outcome| matches!(outcome.result, HookResult::Failed(_)))
        .collect();

    // The point of the whole exercise: the next hang names its own cause.
    for outcome in &overran {
        crate::teprintln!(
            "[lifecycle] {} exceeded its {}ms budget and was abandoned while {}",
            outcome.module,
            outcome.budget.as_millis(),
            outcome.detail
        );
    }
    for outcome in outcomes.iter().filter(|o| o.needs_verification) {
        crate::teprintln!(
            "[lifecycle] {} was interrupted somewhere it considers unsafe ({}); it should verify its own state on the next start",
            outcome.module,
            outcome.detail
        );
    }
    for outcome in &failed {
        if let HookResult::Failed(error) = &outcome.result {
            crate::teprintln!("[lifecycle] {} reported an error: {error}", outcome.module);
        }
    }

    crate::tprintln!(
        "[lifecycle] stopped in {}ms: {} ok, {} failed, {} overran, {} skipped",
        elapsed.as_millis(),
        outcomes
            .iter()
            .filter(|o| matches!(o.result, HookResult::Ok))
            .count(),
        failed.len(),
        overran.len(),
        outcomes
            .iter()
            .filter(|o| matches!(o.result, HookResult::Skipped(_)))
            .count(),
    );
}

/// Emits the opt-in timing report as a GUI marker.
///
/// The daemon emits, the Kotlin layer copies. Clipboard access is an Android API and reaching
/// for it from here would widen the JNI surface for no benefit; the log poller already handles
/// `GUI_` markers.
///
/// Emitted before the `After` tier tears the bridge down, so the slowest shutdowns - the ones
/// actually worth reading - are not the ones whose report gets lost.
fn emit_timing_report(outcomes: &[HookOutcome], elapsed: Duration) {
    let mut body = format!("total={}ms\n", elapsed.as_millis());
    for outcome in outcomes {
        body.push_str(&format!("{outcome}\n"));
    }
    crate::tprintln!("[gui] GUI_SHUTDOWN_TIMINGS={}", hex::encode(body.as_bytes()));
}

// ---------------------------------------------------------------------------
// Watchdog
// ---------------------------------------------------------------------------

/// Hard-exit backstop on its own OS thread.
///
/// Deliberately not a runtime task: a hook doing blocking work occupies a worker, and enough
/// of them means a timer task never gets polled. A thread cannot be starved that way.
struct Watchdog {
    disarmed: Arc<std::sync::atomic::AtomicBool>,
}

impl Watchdog {
    fn arm(
        limit: Duration,
        action: WatchdogAction,
        fired: Arc<std::sync::atomic::AtomicBool>,
        data_dir: Option<std::path::PathBuf>,
        progress: Arc<std::sync::Mutex<Vec<HookOutcome>>>,
        reason: StopReason,
    ) -> Self {
        let disarmed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = disarmed.clone();
        std::thread::Builder::new()
            .name("veilknit-shutdown-watchdog".into())
            .spawn(move || {
                let deadline = Instant::now() + limit;
                while Instant::now() < deadline {
                    if flag.load(std::sync::atomic::Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(10).min(limit / 4));
                }
                if flag.load(std::sync::atomic::Ordering::Relaxed) {
                    return;
                }
                fired.store(true, std::sync::atomic::Ordering::Relaxed);

                // The important copy. A shutdown that completed cleanly is the one nobody
                // needs to investigate; this is the one that vanishes without a trace unless
                // it is written here, before the exit.
                if let Some(dir) = &data_dir {
                    let collected = progress
                        .lock()
                        .map(|guard| guard.clone())
                        .unwrap_or_default();
                    ShutdownRecord::build(reason, true, limit, &collected).write(dir);
                }

                match action {
                    WatchdogAction::Exit => {
                        eprintln!(
                            "[lifecycle] graceful shutdown exceeded {}s; forcing exit",
                            limit.as_secs()
                        );
                        std::process::exit(0);
                    }
                    WatchdogAction::Report => {
                        eprintln!(
                            "[lifecycle] watchdog would have forced exit after {}ms",
                            limit.as_millis()
                        );
                    }
                }
            })
            .ok();
        Self { disarmed }
    }

    fn disarm(&self) {
        self.disarmed
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needs_map_to_tiers() {
        let hook = |need| Hook {
            module: "test".into(),
            detail: String::new(),
            need,
            budget: budget::DEFAULT_HOOK,
            optional: false,
            action: Box::new(|| Box::pin(async { Ok(()) })),
        };
        assert_eq!(hook(ResourceNeed::None).tier(false), Tier::Intake);
        assert_eq!(hook(ResourceNeed::Network).tier(false), Tier::Announce);
        assert_eq!(hook(ResourceNeed::VeilidNode).tier(false), Tier::NodeDependent);
        assert_eq!(hook(ResourceNeed::Storage).tier(false), Tier::Persist);
        assert_eq!(hook(ResourceNeed::VeilidNode).tier(true), Tier::Teardown);
    }

    #[test]
    fn tiers_are_ordered() {
        assert!(Tier::Intake < Tier::Announce);
        assert!(Tier::Announce < Tier::NodeDependent);
        assert!(Tier::Persist < Tier::Teardown);
        assert!(Tier::Teardown < Tier::After);
    }

    #[tokio::test]
    async fn overrunning_hook_is_recorded_not_awaited() {
        let lifecycle = Lifecycle::with(Budgets::default().scaled(0.02), WatchdogAction::Report);
        lifecycle
            .register(
                HookSpec::new("slow", ResourceNeed::None).budget(Duration::from_millis(50)),
                || async {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    Ok(())
                },
            )
            .await;

        let started = Instant::now();
        let outcomes = lifecycle
            .stop(StopReason::Shutdown, Reachability::Unreachable, async {
                Reachability::Unreachable
            })
            .await;

        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].result, HookResult::Overran);
    }

    #[tokio::test]
    async fn announce_tier_is_skipped_when_unreachable() {
        let lifecycle = Lifecycle::with(Budgets::default().scaled(0.02), WatchdogAction::Report);
        lifecycle
            .register(HookSpec::new("presence", ResourceNeed::Network), || async {
                panic!("announce hook must not run when the network is unreachable")
            })
            .await;

        let outcomes = lifecycle
            .stop(StopReason::Shutdown, Reachability::Unreachable, async {
                Reachability::Unreachable
            })
            .await;

        assert_eq!(
            outcomes[0].result,
            HookResult::Skipped("network unreachable")
        );
    }

    #[tokio::test]
    async fn restart_skips_announce_even_when_reachable() {
        let lifecycle = Lifecycle::with(Budgets::default().scaled(0.02), WatchdogAction::Report);
        lifecycle
            .register(HookSpec::new("presence", ResourceNeed::Network), || async {
                panic!("restart must not announce")
            })
            .await;

        let outcomes = lifecycle
            .stop(StopReason::Restart, Reachability::Reachable, async {
                Reachability::Reachable
            })
            .await;

        assert_eq!(outcomes[0].result, HookResult::Skipped("restart"));
    }

    #[tokio::test]
    async fn second_stop_is_ignored() {
        let lifecycle = Lifecycle::with(Budgets::default().scaled(0.02), WatchdogAction::Report);
        lifecycle
            .register(HookSpec::new("once", ResourceNeed::None), || async { Ok(()) })
            .await;

        let first = lifecycle
            .stop(StopReason::Shutdown, Reachability::Unreachable, async {
                Reachability::Unreachable
            })
            .await;
        let second = lifecycle
            .stop(StopReason::Shutdown, Reachability::Unreachable, async {
                Reachability::Unreachable
            })
            .await;

        assert_eq!(first.len(), 1);
        assert!(second.is_empty());
    }
}
