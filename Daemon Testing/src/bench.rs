//! Scenario runner for the test bench.
//!
//! Drives the lifecycle coordinator from declared JSON scenarios and checks the declared
//! expectations, so a run is pass/fail rather than something both of us have to read end to
//! end. Successes are logged as fully as failures: a hook reporting "done" while the event
//! stream shows nothing happened is exactly the class of bug a bare verdict hides.
//!
//! Timestamps are wall clock, with the unix start time in the header, so two nodes' logs line
//! up against each other by subtraction.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::Deserialize;
use serde_json::Value;

use crate::{
    events::network_events::StartupStage,
    lifecycle::{
        Budgets, HookResult, HookSpec, Lifecycle, Reachability, ResourceNeed, StageOutcome,
        StopReason, WatchdogAction,
    },
};

// ---------------------------------------------------------------------------
// Scenario format
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct Scenario {
    name: String,
    #[serde(default)]
    description: String,
    /// Scales every tier ceiling so overrun paths finish in milliseconds. 1.0 is production.
    #[serde(default = "default_budget_scale")]
    budget_scale: f64,
    #[serde(default = "default_reason")]
    reason: String,
    #[serde(default = "default_reachability")]
    reachability: String,
    #[serde(default)]
    hooks: Vec<HookDef>,
    /// Stages marked ready or unavailable after a delay, for startup gating scenarios.
    #[serde(default)]
    stages: Vec<StageDef>,
    /// Modules that wait on a stage before the stop begins.
    #[serde(default)]
    waiters: Vec<WaiterDef>,
    /// Starts a task that ticks on a fixed interval for the duration of the stop.
    ///
    /// The only direct evidence of runtime starvation. A `tokio::time::sleep` that does not
    /// fire proves the time driver was never polled, which cannot be inferred from hook
    /// timings alone.
    #[serde(default)]
    canary: bool,
    #[serde(default)]
    expect: Expectations,
}

fn default_budget_scale() -> f64 {
    0.02
}
fn default_reason() -> String {
    "shutdown".into()
}
fn default_reachability() -> String {
    "reachable".into()
}

#[derive(Debug, Deserialize, Clone)]
struct HookDef {
    module: String,
    need: String,
    #[serde(default)]
    behaviour: Behaviour,
    /// Milliseconds the behaviour occupies.
    #[serde(default)]
    ms: u64,
    /// Per-hook budget in milliseconds, before scaling. Omitted means the tier default.
    #[serde(default)]
    budget_ms: Option<u64>,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
enum Behaviour {
    #[default]
    Ok,
    /// Returns an error rather than hanging.
    Error,
    /// Occupies a blocking-pool thread with a synchronous sleep. Cannot be cancelled, so it
    /// proves overrun is judged on measured time rather than on the timeout firing - but it
    /// starves nothing, because the blocking pool is separate from the workers.
    BlockThread,
    /// Occupies a runtime WORKER thread, not the blocking pool.
    ///
    /// The only behaviour that can still starve the runtime now hooks run via spawn_blocking,
    /// and therefore the only way to reach the watchdog at all. Without this the watchdog is
    /// untestable and we would be trusting code no scenario exercises.
    StarveWorker,
    /// Panics inside the hook, to prove one bad module does not take the tier down.
    Panic,
}

#[derive(Debug, Deserialize, Clone)]
struct StageDef {
    stage: String,
    /// `ready` or `unavailable`.
    #[serde(default = "default_stage_outcome")]
    outcome: String,
    #[serde(default)]
    after_ms: u64,
}

fn default_stage_outcome() -> String {
    "ready".into()
}

#[derive(Debug, Deserialize, Clone)]
struct WaiterDef {
    module: String,
    stage: String,
    /// Ceiling in milliseconds. Zero waits indefinitely.
    #[serde(default)]
    timeout_ms: u64,
}

#[derive(Debug, Deserialize, Default)]
struct Expectations {
    #[serde(default)]
    watchdog_fired: Option<bool>,
    #[serde(default)]
    total_under_ms: Option<u128>,
    /// module -> expected result: ok | failed | overran | skipped
    #[serde(default)]
    hooks: BTreeMap<String, String>,
    /// module -> expected stage outcome: ready | unavailable | pending
    #[serde(default)]
    waiters: BTreeMap<String, String>,
    /// Modules that must not have run at all.
    #[serde(default)]
    never_ran: Vec<String>,
    /// module -> substring its recorded detail must contain. Checks the abandoned hook's last
    /// reported step survived, which is the whole reason Progress exists.
    #[serde(default)]
    details: BTreeMap<String, String>,
    /// Modules that must have flagged themselves as needing verification.
    #[serde(default)]
    needs_verification: Vec<String>,
    /// Longest gap the canary is allowed between ticks. A gap far larger than the interval
    /// means the runtime was starved.
    #[serde(default)]
    canary_max_gap_ms: Option<u128>,
}

// ---------------------------------------------------------------------------
// Logging
// ---------------------------------------------------------------------------

/// Wall-clock millisecond timestamp, so logs from two nodes line up.
fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn stamp() -> String {
    let ms = now_ms();
    format!("{}.{:03}", ms / 1000, ms % 1000)
}

#[derive(Clone, Default)]
struct EventLog {
    lines: Arc<Mutex<Vec<String>>>,
}

impl EventLog {
    fn record(&self, source: &str, message: impl AsRef<str>) {
        let line = format!("[{}] [{:<14}] {}", stamp(), source, message.as_ref());
        println!("{line}");
        if let Ok(mut lines) = self.lines.lock() {
            lines.push(line);
        }
    }

    fn contains(&self, needle: &str) -> bool {
        self.lines
            .lock()
            .map(|lines| lines.iter().any(|line| line.contains(needle)))
            .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// Running one scenario
// ---------------------------------------------------------------------------

/// How often the canary ticks while a stop is in progress.
const CANARY_INTERVAL: Duration = Duration::from_millis(50);

/// Evidence collected by the canary task.
#[derive(Default)]
struct CanaryReport {
    ticks: u64,
    longest_gap: Duration,
}

/// Ticks on a timer for the duration of the stop and records the longest gap between ticks.
///
/// This is the measurement that separates "the timeout did not fire" from "nothing could
/// run". If the gap stays near [`CANARY_INTERVAL`] the runtime kept working and a missed
/// timeout has some other cause; if it balloons to the length of a blocking hook, the time
/// driver was never polled and every timeout in flight was helpless by construction.
fn spawn_canary(log: EventLog) -> (tokio::task::JoinHandle<()>, Arc<Mutex<CanaryReport>>) {
    let report = Arc::new(Mutex::new(CanaryReport::default()));
    let collected = report.clone();

    let handle = tokio::spawn(async move {
        let mut last = std::time::Instant::now();
        loop {
            tokio::time::sleep(CANARY_INTERVAL).await;
            let now = std::time::Instant::now();
            let gap = now.duration_since(last);
            last = now;

            let mut guard = collected.lock().unwrap();
            guard.ticks += 1;
            if gap > guard.longest_gap {
                guard.longest_gap = gap;
            }
            let ticks = guard.ticks;
            drop(guard);

            // Only the surprising ticks are logged. A tick every 50ms for a minute would bury
            // everything else; a tick that arrived a minute late is the whole point.
            if gap > CANARY_INTERVAL * 4 {
                log.record(
                    "canary",
                    format!("tick {ticks} arrived {}ms after the previous one", gap.as_millis()),
                );
            }
        }
    });

    (handle, report)
}

struct Verdict {
    name: String,
    failures: Vec<String>,
}

async fn run_scenario(scenario: Scenario) -> Verdict {
    let log = EventLog::default();
    let budgets = Budgets::default().scaled(scenario.budget_scale);
    let lifecycle = Lifecycle::with(budgets, WatchdogAction::Report);

    println!("\n{}", "=".repeat(78));
    println!("SCENARIO   {}", scenario.name);
    if !scenario.description.is_empty() {
        println!("           {}", scenario.description);
    }
    println!(
        "START      {} (unix ms {})",
        stamp(),
        now_ms()
    );
    println!(
        "CONFIG     reason={} reachability={} budget_scale={} watchdog={}ms",
        scenario.reason,
        scenario.reachability,
        scenario.budget_scale,
        budgets.watchdog.as_millis()
    );
    println!("EXPECT     {}", describe_expectations(&scenario.expect));
    println!("{}", "-".repeat(78));

    // Counts every actual invocation, so `never_ran` is checked against what happened rather
    // than against what the coordinator reported.
    let invocations: Arc<Mutex<BTreeMap<String, u64>>> = Arc::new(Mutex::new(BTreeMap::new()));

    for hook in &scenario.hooks {
        let def = hook.clone();
        let log = log.clone();
        let invocations = invocations.clone();
        let mut spec = HookSpec::new(def.module.clone(), parse_need(&def.need))
            .detail(format!("{:?} {}ms", def.behaviour, def.ms));
        if let Some(budget_ms) = def.budget_ms {
            spec = spec.budget(scale_ms(budget_ms, scenario.budget_scale));
        }

        lifecycle
            .register(spec, move |progress| {
                let def = def.clone();
                let log = log.clone();
                let invocations = invocations.clone();
                async move {
                    *invocations
                        .lock()
                        .unwrap()
                        .entry(def.module.clone())
                        .or_insert(0) += 1;
                    log.record(&def.module, format!("hook entered ({:?})", def.behaviour));
                    progress.step(format!("{:?} for {}ms", def.behaviour, def.ms));

                    match def.behaviour {
                        Behaviour::Ok => {
                            // Split into halves so an abandoned hook's recorded step is a
                            // phase rather than the whole hook - the point of the mechanism.
                            progress.step("first half");
                            tokio::time::sleep(Duration::from_millis(def.ms / 2)).await;
                            progress.fragile("second half, mid-write");
                            tokio::time::sleep(Duration::from_millis(def.ms - def.ms / 2)).await;
                            progress.settled("finished cleanly");
                            log.record(&def.module, "work complete");
                            Ok(())
                        }
                        Behaviour::Error => {
                            tokio::time::sleep(Duration::from_millis(def.ms)).await;
                            log.record(&def.module, "reporting failure");
                            Err("mock failure".to_string())
                        }
                        Behaviour::BlockThread => {
                            progress.fragile("blocking, cannot be interrupted");
                            log.record(&def.module, "occupying a worker thread");
                            std::thread::sleep(Duration::from_millis(def.ms));
                            log.record(&def.module, "released the worker thread");
                            Ok(())
                        }
                        Behaviour::StarveWorker => {
                            // Spawned onto the runtime rather than run here: this hook is
                            // itself on a blocking thread, so blocking again would starve
                            // nothing. The spawned task is what occupies a worker.
                            progress.fragile("starving a runtime worker");
                            log.record(&def.module, "starving a runtime worker");
                            let handle = tokio::runtime::Handle::current();
                            let ms = def.ms;
                            let occupied = handle.spawn(async move {
                                std::thread::sleep(Duration::from_millis(ms));
                            });
                            let _ = occupied.await;
                            log.record(&def.module, "released the runtime worker");
                            Ok(())
                        }
                        Behaviour::Panic => {
                            log.record(&def.module, "about to panic");
                            panic!("mock panic from {}", def.module)
                        }
                    }
                }
            })
            .await;
    }

    // Stage drivers: mark stages after their delay, so waiters have something to wake on.
    let mut drivers = Vec::new();
    for stage_def in &scenario.stages {
        let Some(stage) = parse_stage(&stage_def.stage) else {
            log.record("bench", format!("unknown stage {}", stage_def.stage));
            continue;
        };
        let gates = lifecycle.gates().clone();
        let log = log.clone();
        let def = stage_def.clone();
        drivers.push(tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(def.after_ms)).await;
            if def.outcome == "unavailable" {
                gates.mark_unavailable(stage);
                log.record("stage", format!("{:?} -> unavailable", stage));
            } else {
                gates.mark_ready(stage);
                log.record("stage", format!("{:?} -> ready", stage));
            }
        }));
    }

    // Waiters run before the stop, exercising startup gating.
    let waiter_results: Arc<Mutex<BTreeMap<String, StageOutcome>>> =
        Arc::new(Mutex::new(BTreeMap::new()));
    let mut waiter_tasks = Vec::new();
    for waiter in &scenario.waiters {
        let Some(stage) = parse_stage(&waiter.stage) else {
            log.record("bench", format!("unknown stage {}", waiter.stage));
            continue;
        };
        let gate = lifecycle.gates().gate(stage);
        let log = log.clone();
        let results = waiter_results.clone();
        let def = waiter.clone();
        waiter_tasks.push(tokio::spawn(async move {
            log.record(&def.module, format!("waiting on {:?}", stage));
            let outcome = if def.timeout_ms == 0 {
                gate.wait().await
            } else {
                gate.wait_for(Duration::from_millis(def.timeout_ms)).await
            };
            log.record(&def.module, format!("released as {:?}", outcome));
            results.lock().unwrap().insert(def.module.clone(), outcome);
        }));
    }
    for task in waiter_tasks {
        let _ = task.await;
    }

    let canary = if scenario.canary {
        log.record(
            "canary",
            format!("ticking every {}ms for the duration of the stop", CANARY_INTERVAL.as_millis()),
        );
        Some(spawn_canary(log.clone()))
    } else {
        None
    };

    let refreshes = Arc::new(AtomicU64::new(0));
    let refresh_counter = refreshes.clone();
    let refresh_log = log.clone();

    let started = std::time::Instant::now();
    let outcomes = lifecycle
        .stop(
            parse_reason(&scenario.reason),
            parse_reachability(&scenario.reachability),
            async move {
                refresh_counter.fetch_add(1, Ordering::Relaxed);
                refresh_log.record("bench", "resolving unknown reachability with a cheap read");
                Reachability::Unreachable
            },
        )
        .await;
    let elapsed = started.elapsed();

    for driver in drivers {
        driver.abort();
    }

    let canary_report = canary.map(|(handle, report)| {
        handle.abort();
        let guard = report.lock().unwrap();
        CanaryReport { ticks: guard.ticks, longest_gap: guard.longest_gap }
    });

    // --- summary table. This is the part worth diffing between runs; the event stream above
    // interleaves differently every time because tiers run concurrently.
    println!("{}", "-".repeat(78));
    println!("SUMMARY");
    let mut sorted = outcomes.clone();
    sorted.sort_by(|a, b| a.tier.cmp(&b.tier).then(a.module.cmp(&b.module)));
    for outcome in &sorted {
        println!("  {outcome}");
    }
    println!(
        "  {:<10} {:<26} {:>6}ms",
        "total", "(wall)", elapsed.as_millis()
    );
    if refreshes.load(Ordering::Relaxed) > 0 {
        println!("  reachability resolved by cheap read");
    }
    if let Some(report) = &canary_report {
        let expected = elapsed.as_millis() / CANARY_INTERVAL.as_millis().max(1);
        println!(
            "  canary    {} tick(s), expected about {expected}, longest gap {}ms",
            report.ticks,
            report.longest_gap.as_millis()
        );
        if report.longest_gap > CANARY_INTERVAL * 4 {
            println!(
                "  RUNTIME STARVED: nothing could be polled for {}ms, so no timeout in flight could fire",
                report.longest_gap.as_millis()
            );
        }
    }

    let failures = check(
        &scenario,
        &outcomes,
        elapsed,
        &lifecycle,
        &waiter_results,
        &invocations,
        canary_report.as_ref(),
    );
    println!("{}", "-".repeat(78));
    if failures.is_empty() {
        println!("VERDICT    pass");
    } else {
        println!("VERDICT    FAIL");
        for failure in &failures {
            println!("           {failure}");
        }
    }

    Verdict {
        name: scenario.name,
        failures,
    }
}

fn check(
    scenario: &Scenario,
    outcomes: &[crate::lifecycle::HookOutcome],
    elapsed: Duration,
    lifecycle: &Lifecycle,
    waiters: &Arc<Mutex<BTreeMap<String, StageOutcome>>>,
    invocations: &Arc<Mutex<BTreeMap<String, u64>>>,
    canary: Option<&CanaryReport>,
) -> Vec<String> {
    let mut failures = Vec::new();

    if let Some(expected) = scenario.expect.watchdog_fired {
        let actual = lifecycle.watchdog_fired();
        if actual != expected {
            failures.push(format!(
                "watchdog_fired expected {expected}, observed {actual}"
            ));
        }
    }

    if let Some(limit) = scenario.expect.total_under_ms {
        if elapsed.as_millis() > limit {
            failures.push(format!(
                "took {}ms, expected under {}ms",
                elapsed.as_millis(),
                limit
            ));
        }
    }

    for (module, expected) in &scenario.expect.hooks {
        match outcomes.iter().find(|o| &o.module == module) {
            None => failures.push(format!("{module} produced no outcome at all")),
            Some(outcome) => {
                let actual = match &outcome.result {
                    HookResult::Ok => "ok",
                    HookResult::Failed(_) => "failed",
                    HookResult::Overran => "overran",
                    HookResult::Skipped(_) => "skipped",
                };
                if actual != expected {
                    failures.push(format!("{module} expected {expected}, observed {actual}"));
                }
            }
        }
    }

    for (module, expected) in &scenario.expect.waiters {
        let observed = waiters.lock().unwrap().get(module).copied();
        let actual = match observed {
            Some(StageOutcome::Ready) => "ready",
            Some(StageOutcome::Unavailable) => "unavailable",
            Some(StageOutcome::Pending) => "pending",
            None => "absent",
        };
        if actual != expected {
            failures.push(format!(
                "waiter {module} expected {expected}, observed {actual}"
            ));
        }
    }

    // Checked against real invocations rather than reported outcomes: a hook that was skipped
    // and a hook that ran and lied both report cleanly, and only one of them is correct.
    for module in &scenario.expect.never_ran {
        if let Some(count) = invocations.lock().unwrap().get(module) {
            failures.push(format!("{module} ran {count} time(s) but should not have"));
        }
    }

    for (module, fragment) in &scenario.expect.details {
        match outcomes.iter().find(|o| &o.module == module) {
            None => failures.push(format!("{module} produced no outcome to check detail on")),
            Some(outcome) if !outcome.detail.contains(fragment.as_str()) => failures.push(
                format!("{module} detail was \"{}\", expected it to contain \"{fragment}\"", outcome.detail),
            ),
            Some(_) => {}
        }
    }

    for module in &scenario.expect.needs_verification {
        match outcomes.iter().find(|o| &o.module == module) {
            None => failures.push(format!("{module} produced no outcome to check verification on")),
            Some(outcome) if !outcome.needs_verification => {
                failures.push(format!("{module} did not flag itself as needing verification"))
            }
            Some(_) => {}
        }
    }

    if let Some(limit) = scenario.expect.canary_max_gap_ms {
        match canary {
            None => failures.push(
                "canary_max_gap_ms was declared but the scenario did not enable the canary".into(),
            ),
            Some(report) if report.longest_gap.as_millis() > limit => failures.push(format!(
                "runtime was starved for {}ms, expected no gap over {}ms",
                report.longest_gap.as_millis(),
                limit
            )),
            Some(_) => {}
        }
    }

    failures
}

fn describe_expectations(expect: &Expectations) -> String {
    let mut parts = Vec::new();
    if let Some(value) = expect.watchdog_fired {
        parts.push(format!("watchdog_fired={value}"));
    }
    if let Some(value) = expect.total_under_ms {
        parts.push(format!("total<{value}ms"));
    }
    for (module, result) in &expect.hooks {
        parts.push(format!("{module}={result}"));
    }
    for (module, result) in &expect.waiters {
        parts.push(format!("waiter:{module}={result}"));
    }
    for module in &expect.never_ran {
        parts.push(format!("{module}=never_ran"));
    }
    if let Some(value) = expect.canary_max_gap_ms {
        parts.push(format!("canary_gap<{value}ms"));
    }
    for (module, fragment) in &expect.details {
        parts.push(format!("{module}~\"{fragment}\""));
    }
    for module in &expect.needs_verification {
        parts.push(format!("{module}=needs_verification"));
    }
    if parts.is_empty() {
        "(none declared)".into()
    } else {
        parts.join("  ")
    }
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

fn scale_ms(ms: u64, factor: f64) -> Duration {
    Duration::from_secs_f64(((ms as f64 / 1000.0) * factor).max(0.001))
}

fn parse_need(value: &str) -> ResourceNeed {
    match value.to_ascii_lowercase().as_str() {
        "network" => ResourceNeed::Network,
        "veilidnode" | "veilid_node" | "node" => ResourceNeed::VeilidNode,
        "storage" => ResourceNeed::Storage,
        _ => ResourceNeed::None,
    }
}

fn parse_reason(value: &str) -> StopReason {
    match value.to_ascii_lowercase().as_str() {
        "restart" => StopReason::Restart,
        _ => StopReason::Shutdown,
    }
}

fn parse_reachability(value: &str) -> Reachability {
    match value.to_ascii_lowercase().as_str() {
        "unreachable" => Reachability::Unreachable,
        "unknown" => Reachability::Unknown,
        _ => Reachability::Reachable,
    }
}

fn parse_stage(value: &str) -> Option<StartupStage> {
    let normalised = value.to_ascii_lowercase().replace(['_', '-'], "");
    let candidates = [
        (StartupStage::Configuration, "configuration"),
        (StartupStage::Identity, "identity"),
        (StartupStage::Reputation, "reputation"),
        (StartupStage::Veilid, "veilid"),
        (StartupStage::NetworkAttachment, "networkattachment"),
        (StartupStage::DhtRestore, "dhtrestore"),
        (StartupStage::MainDht, "maindht"),
        (StartupStage::DhtNetworkVerification, "dhtnetworkverification"),
        (StartupStage::Presence, "presence"),
        (StartupStage::Routes, "routes"),
        (StartupStage::Handshake, "handshake"),
        (StartupStage::Mailbox, "mailbox"),
        (StartupStage::Walker, "walker"),
        (StartupStage::ApplicationInfo, "applicationinfo"),
        (StartupStage::BackgroundServices, "backgroundservices"),
        (StartupStage::Ready, "ready"),
    ];
    candidates
        .iter()
        .find(|(_, name)| *name == normalised)
        .map(|(stage, _)| *stage)
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn collect_scenarios(path: &Path) -> Vec<PathBuf> {
    if path.is_file() {
        return vec![path.to_path_buf()];
    }
    let mut found: Vec<PathBuf> = fs::read_dir(path)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
                .collect()
        })
        .unwrap_or_default();
    found.sort();
    found
}

/// Lists scenarios without running them.
pub fn list_scenarios(target: &Path) {
    for path in collect_scenarios(target) {
        match load(&path) {
            Ok(scenario) => println!("{:<32} {}", scenario.name, scenario.description),
            Err(error) => println!("{:<32} MALFORMED: {error}", path.display()),
        }
    }
}

fn load(path: &Path) -> Result<Scenario, String> {
    let raw = fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str::<Value>(&raw)
        .and_then(serde_json::from_value)
        .map_err(|error| error.to_string())
}

/// Runs every scenario at `target` and returns the number that failed.
///
/// Mock modules only: hooks are closures built from the scenario, not the daemon's real
/// modules. That is deliberate for now - it proves the coordinator sequences correctly. Real
/// module behaviour needs live mode and a second node reading records back, because a module
/// reporting "written" and a module that actually wrote look identical from in here.
/// Worker threads for scenario mode.
///
/// Fixed rather than inherited from the host. `#[tokio::main]` gives one worker per core, so
/// how many blocking hooks it takes to starve the runtime would vary by machine, and
/// shutdown-blocking-hook would mean something different on every box. Two is enough to show
/// concurrency within a tier and small enough that three blocking hooks can starve it.
const SCENARIO_WORKER_THREADS: usize = 2;

/// How long to wait for abandoned blocking work before dropping the runtime anyway.
///
/// Short on purpose. Anything still running here has already been recorded as having overrun,
/// so waiting on it changes no result - it only delays the exit.
const RUNTIME_SHUTDOWN_GRACE: Duration = Duration::from_millis(250);

/// Runs the scenarios on a dedicated runtime and returns the number that failed.
///
/// Built on its own thread because `main` is already inside a runtime and tokio refuses to
/// build one from within another.
pub fn run_scenarios(target: &Path) -> i32 {
    let target = target.to_path_buf();
    let spawned = std::thread::Builder::new()
        .name("scenario-runtime".into())
        .spawn(move || {
            match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(SCENARIO_WORKER_THREADS)
                .enable_all()
                .build()
            {
                Ok(runtime) => {
                    let failed = runtime.block_on(run_all(&target));
                    // Dropping a runtime waits for its blocking tasks, so a hook the
                    // coordinator correctly abandoned would still hold the process open until
                    // it finished on its own. Abandoning means abandoning.
                    runtime.shutdown_timeout(RUNTIME_SHUTDOWN_GRACE);
                    failed
                }
                Err(error) => {
                    eprintln!("could not build the scenario runtime: {error}");
                    1
                }
            }
        });

    match spawned {
        Ok(handle) => handle.join().unwrap_or(1),
        Err(error) => {
            eprintln!("could not start the scenario thread: {error}");
            1
        }
    }
}

async fn run_all(target: &Path) -> i32 {
    let paths = collect_scenarios(target);
    if paths.is_empty() {
        eprintln!("No scenarios found at {}", target.display());
        return 1;
    }

    let mut verdicts = Vec::new();
    for path in paths {
        let scenario = match load(&path) {
            Ok(scenario) => scenario,
            Err(error) => {
                eprintln!("{}: {error}", path.display());
                verdicts.push(Verdict {
                    name: path.display().to_string(),
                    failures: vec![error],
                });
                continue;
            }
        };
        verdicts.push(run_scenario(scenario).await);
    }

    println!("\n{}", "=".repeat(78));
    println!("RESULTS");
    let mut failed = 0;
    for verdict in &verdicts {
        if verdict.failures.is_empty() {
            println!("  pass  {}", verdict.name);
        } else {
            failed += 1;
            println!("  FAIL  {}  ({} issue(s))", verdict.name, verdict.failures.len());
        }
    }
    println!("{} scenario(s), {failed} failed", verdicts.len());
    failed
}
