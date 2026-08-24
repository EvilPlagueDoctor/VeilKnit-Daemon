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
    /// Occupies a runtime worker with a synchronous sleep. This is the shape a timeout
    /// expressed as a task cannot interrupt, and the reason the watchdog is a thread.
    BlockThread,
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
            .register(spec, move || {
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

                    match def.behaviour {
                        Behaviour::Ok => {
                            tokio::time::sleep(Duration::from_millis(def.ms)).await;
                            log.record(&def.module, "work complete");
                            Ok(())
                        }
                        Behaviour::Error => {
                            tokio::time::sleep(Duration::from_millis(def.ms)).await;
                            log.record(&def.module, "reporting failure");
                            Err("mock failure".to_string())
                        }
                        Behaviour::BlockThread => {
                            log.record(&def.module, "occupying a worker thread");
                            std::thread::sleep(Duration::from_millis(def.ms));
                            log.record(&def.module, "released the worker thread");
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

    let failures = check(&scenario, &outcomes, elapsed, &lifecycle, &waiter_results, &invocations);
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
pub async fn run_scenarios(target: &Path) -> i32 {
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
