use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use veilid_core::{RouteId, Target, VeilidAPI, VeilidAPIResult};

use crate::dht_module::DHTModule;
use crate::node::Node;
use crate::types::{current_timestamp, RouteBlobRecord, BLOB_LOCATION};

// ============================================================================
// Readiness state
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    WaitingForNode,
    WaitingForDht,
    Ready,
}

// ============================================================================
// Publish lifecycle
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishState {
    Idle,
    Publishing,
}

// ============================================================================
// Status snapshot, handed back to callers who want to check on things
// (e.g. from a UI) without poking at the private Inner state directly.
// ============================================================================

#[derive(Debug, Clone)]
pub struct RouteManagerStatus {
    pub readiness: Readiness,
    pub publish_state: PublishState,
    pub active_route_id: Option<RouteId>,
}

impl RouteManagerStatus {
    fn unknown() -> Self {
        Self {
            readiness: Readiness::WaitingForNode,
            publish_state: PublishState::Idle,
            active_route_id: None,
        }
    }
}

// ============================================================================
// Messages
// ============================================================================

enum RouteManagerMsg {
    SetNode(Arc<Node>),

    /// The new DHTModule stores DHT records internally and refers to them by
    /// package index, not by RecordKey + external DhtStore.
    SetDht {
        dht_module: DHTModule,
        dht_package: usize,
    },

    RouteChange(Vec<RouteId>),

    GetStatus(oneshot::Sender<RouteManagerStatus>),
}

// ============================================================================
// Public handle
// ============================================================================

#[derive(Clone)]
pub struct RouteManager {
    tx: mpsc::Sender<RouteManagerMsg>,
}

impl RouteManager {
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel(64);
        tokio::spawn(route_manager_task(rx));
        Self { tx }
    }

    pub async fn set_node(&self, node: Arc<Node>) {
        if let Err(e) = self.tx.send(RouteManagerMsg::SetNode(node)).await {
            println!("[route_manager] Failed to send SetNode: {e}");
        }
    }

    /// Give the manager the DHT package it should publish our current
    /// private-route blob to.
    ///
    /// `dht_package` is the usize index returned by `DHTModule::create_dht(...)`
    /// or the index assigned after importing/restoring a DHT snapshot.
    pub async fn set_dht(&self, dht_module: DHTModule, dht_package: usize) {
        if let Err(e) = self
            .tx
            .send(RouteManagerMsg::SetDht {
                dht_module,
                dht_package,
            })
            .await
        {
            println!("[route_manager] Failed to send SetDht: {e}");
        }
    }

    pub fn make_route_change_handler(
        &self,
    ) -> impl Fn(Vec<RouteId>) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync + 'static
    {
        let tx = self.tx.clone();

        move |dead_routes: Vec<RouteId>| {
            let tx = tx.clone();

            Box::pin(async move {
                let _ = tx.send(RouteManagerMsg::RouteChange(dead_routes)).await;
            })
        }
    }

    /// Ask the background task for a snapshot of where things currently
    /// stand: readiness (waiting on node/DHT, or ready), whether a publish
    /// is in flight, and the currently active route id, if any.
    pub async fn get_status(&self) -> RouteManagerStatus {
        let (reply_tx, reply_rx) = oneshot::channel();

        if self
            .tx
            .send(RouteManagerMsg::GetStatus(reply_tx))
            .await
            .is_err()
        {
            return RouteManagerStatus::unknown();
        }

        reply_rx.await.unwrap_or_else(|_| RouteManagerStatus::unknown())
    }

    /// Send one app message to a published remote private-route blob.
    ///
    /// `route_blob` is the bytes stored in `RouteBlobRecord.blob`.
    pub async fn send_private_route_message(
        veilid: &VeilidAPI,
        route_blob: &[u8],
        data: Vec<u8>,
    ) -> VeilidAPIResult<()> {
        let route_id = veilid.import_remote_private_route(route_blob.to_vec())?;
        let routing_context = veilid.routing_context()?;

        let result = routing_context
            .app_message(Target::RouteId(route_id.clone()), data)
            .await;

        let _ = veilid.release_private_route(route_id);

        result
    }
}

// ============================================================================
// Internal state
// ============================================================================

struct Inner {
    readiness: Readiness,
    publish_state: PublishState,

    node: Option<Arc<Node>>,
    dht_module: Option<DHTModule>,
    dht_package: Option<usize>,

    active_route_id: Option<RouteId>,

    /// If true, one of our routes died while a publish operation was already
    /// running. After publish completes, immediately publish again.
    pending_route_loss: bool,
}

impl Inner {
    fn new() -> Self {
        Self {
            readiness: Readiness::WaitingForNode,
            publish_state: PublishState::Idle,
            node: None,
            dht_module: None,
            dht_package: None,
            active_route_id: None,
            pending_route_loss: false,
        }
    }

    fn refresh_readiness(&mut self) {
        self.readiness = match (&self.node, &self.dht_module, &self.dht_package) {
            (None, _, _) => Readiness::WaitingForNode,
            (Some(_), None, _) | (Some(_), _, None) => Readiness::WaitingForDht,
            (Some(_), Some(_), Some(_)) => Readiness::Ready,
        };
    }

    fn can_publish(&self) -> bool {
        self.readiness == Readiness::Ready && self.publish_state == PublishState::Idle
    }

    fn status(&self) -> RouteManagerStatus {
        RouteManagerStatus {
            readiness: self.readiness,
            publish_state: self.publish_state,
            active_route_id: self.active_route_id.clone(),
        }
    }
}

// ============================================================================
// Background task
// ============================================================================

async fn route_manager_task(mut rx: mpsc::Receiver<RouteManagerMsg>) {
    let mut inner = Inner::new();

    while let Some(msg) = rx.recv().await {
        match msg {
            RouteManagerMsg::SetNode(node) => {
                println!("[route_manager] Node received");
                inner.node = Some(node);
                inner.refresh_readiness();

                if inner.can_publish() {
                    publish_loop(&mut inner).await;
                }
            }

            RouteManagerMsg::SetDht {
                dht_module,
                dht_package,
            } => {
                println!("[route_manager] DHT package received: {dht_package}");
                inner.dht_module = Some(dht_module);
                inner.dht_package = Some(dht_package);
                inner.refresh_readiness();

                if inner.can_publish() {
                    publish_loop(&mut inner).await;
                }
            }

            RouteManagerMsg::RouteChange(dead_routes) => {
                if dead_routes.is_empty() {
                    continue;
                }

                let is_dead = inner
                    .active_route_id
                    .as_ref()
                    .map_or(false, |id| dead_routes.contains(id));

                if !is_dead {
                    continue;
                }

                let active_route = inner.active_route_id.take().unwrap();
                println!("[route_manager] Active route died: {active_route:?}");

                match inner.publish_state {
                    PublishState::Idle => {
                        if inner.can_publish() {
                            publish_loop(&mut inner).await;
                        }
                    }
                    PublishState::Publishing => {
                        println!("[route_manager] Route died during publish, queueing republish");
                        inner.pending_route_loss = true;
                    }
                }
            }

            RouteManagerMsg::GetStatus(reply) => {
                let _ = reply.send(inner.status());
            }
        }
    }

    println!("[route_manager] Channel closed, task exiting");
}

// ============================================================================
// Publish loop
// ============================================================================

async fn publish_loop(inner: &mut Inner) {
    if !inner.can_publish() {
        return;
    }

    inner.publish_state = PublishState::Publishing;

    loop {
        inner.pending_route_loss = false;

        let node = match &inner.node {
            Some(n) => Arc::clone(n),
            None => break,
        };

        let dht_module = match &inner.dht_module {
            Some(d) => d.clone(),
            None => break,
        };

        let dht_package = match inner.dht_package {
            Some(i) => i,
            None => break,
        };

        match create_and_publish_route(&node.veilid, &dht_module, dht_package).await {
            Ok(route_id) => {
                println!("[route_manager] Route published: {route_id:?}");
                inner.active_route_id = Some(route_id);

                if inner.pending_route_loss {
                    println!("[route_manager] Route died during publish, republishing");
                    continue;
                }

                break;
            }

            Err(e) => {
                println!("[route_manager] Failed to publish route: {e}");
                inner.active_route_id = None;
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                continue;
            }
        }
    }

    inner.publish_state = PublishState::Idle;
}

async fn create_and_publish_route(
    veilid: &VeilidAPI,
    dht_module: &DHTModule,
    dht_package: usize,
) -> Result<RouteId, String> {
    let route_blob = veilid
        .new_private_route()
        .await
        .map_err(|e| format!("new_private_route failed: {e}"))?;

    let route_id = route_blob.route_id.clone();

    let record = RouteBlobRecord {
        blob: route_blob.blob,
        timestamp: current_timestamp(),
    };

    let bytes = bincode::serialize(&record)
        .map_err(|e| format!("failed to serialize RouteBlobRecord: {e}"))?;

    dht_module
        .write_to_dht(dht_package, BLOB_LOCATION, bytes)
        .await
        .map_err(|e| format!("DHT write failed: {e:?}"))?;

    Ok(route_id)
}