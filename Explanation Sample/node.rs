use arc_swap::ArcSwapOption;
use futures::future::BoxFuture;
use std::sync::Arc;
use tokio::sync::Semaphore;
use veilid_core::*;

/// Generic async handler type
pub type AsyncHandler<T> =
    Arc<dyn Fn(T) -> BoxFuture<'static, ()> + Send + Sync>;

/// Handler aliases
pub type AppMessageHandler = AsyncHandler<Vec<u8>>;
pub type RouteChangeHandler = AsyncHandler<Vec<RouteId>>;
pub type ShutdownHandler = Arc<dyn Fn() -> BoxFuture<'static, ()> + Send + Sync>;
pub type AttachmentHandler = AsyncHandler<VeilidStateAttachment>;
pub type NetworkHandler = Arc<dyn Fn() -> BoxFuture<'static, ()> + Send + Sync>;

/// Internal handler storage
pub struct NodeHandlers {
    pub app_message_handler: ArcSwapOption<AppMessageHandler>,
    pub route_change_handler: ArcSwapOption<RouteChangeHandler>,
    pub shutdown_handler: ArcSwapOption<ShutdownHandler>,
    pub attachment_handler: ArcSwapOption<AttachmentHandler>,
    pub network_handler: ArcSwapOption<NetworkHandler>,
}

impl NodeHandlers {
    pub fn new() -> Self {
        Self {
            app_message_handler: ArcSwapOption::empty(),
            route_change_handler: ArcSwapOption::empty(),
            shutdown_handler: ArcSwapOption::empty(),
            attachment_handler: ArcSwapOption::empty(),
            network_handler: ArcSwapOption::empty(),
        }
    }

    async fn handle_update(&self, update: VeilidUpdate) {
        match update {
            VeilidUpdate::AppMessage(msg) => {
                if let Some(handler) = self.app_message_handler.load().as_ref() {
                    let handler = Arc::clone(handler);

                    let data = msg.message().to_vec();

                    handler(data).await;
                }
            }

            VeilidUpdate::RouteChange(change) => {
                if let Some(handler) = self.route_change_handler.load().as_ref() {
                    let handler = Arc::clone(handler);

                    handler(change.dead_routes).await;
                }
            }

            VeilidUpdate::Shutdown => {
                if let Some(handler) = self.shutdown_handler.load().as_ref() {
                    let handler = Arc::clone(handler);

                    handler().await;
                }
            }

            VeilidUpdate::Attachment(state) => {
                if let Some(handler) = self.attachment_handler.load().as_ref() {
                    let handler = Arc::clone(handler);

                    handler(*state).await;
                }
            }

            VeilidUpdate::Network(_) => {
                if let Some(handler) = self.network_handler.load().as_ref() {
                    let handler = Arc::clone(handler);

                    handler().await;
                }
            }

            _ => {}
        }
    }
}

/// Main node structure
pub struct Node {
    pub veilid: VeilidAPI,

    handlers: Arc<NodeHandlers>,

    /// Limits concurrent update tasks
    update_semaphore: Arc<Semaphore>,
}

impl Node {
    /// Set app message handler
    pub fn set_app_message_handler<F, Fut>(&self, handler: F)
    where
        F: Fn(Vec<u8>) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let wrapped: AppMessageHandler = Arc::new(move |data| {
            Box::pin(handler(data))
        });

        self.handlers
            .app_message_handler
            .store(Some(Arc::new(wrapped)));
    }

    /// Set route change handler
    pub fn set_route_change_handler<F, Fut>(&self, handler: F)
    where
        F: Fn(Vec<RouteId>) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let wrapped: RouteChangeHandler = Arc::new(move |routes| {
            Box::pin(handler(routes))
        });

        self.handlers
            .route_change_handler
            .store(Some(Arc::new(wrapped)));
    }

    /// Set shutdown handler
    pub fn set_shutdown_handler<F, Fut>(&self, handler: F)
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let wrapped: ShutdownHandler = Arc::new(move || {
            Box::pin(handler())
        });

        self.handlers
            .shutdown_handler
            .store(Some(Arc::new(wrapped)));
    }

    /// Set attachment handler
    pub fn set_attachment_handler<F, Fut>(&self, handler: F)
    where
        F: Fn(VeilidStateAttachment) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let wrapped: AttachmentHandler = Arc::new(move |state| {
            Box::pin(handler(state))
        });

        self.handlers
            .attachment_handler
            .store(Some(Arc::new(wrapped)));
    }

    /// Set network handler
    pub fn set_network_handler<F, Fut>(&self, handler: F)
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let wrapped: NetworkHandler = Arc::new(move || {
            Box::pin(handler())
        });

        self.handlers
            .network_handler
            .store(Some(Arc::new(wrapped)));
    }

    /// Shutdown node
    pub async fn shutdown(&self) {
	self.veilid.clone().shutdown().await;
    }

    /// Get state
    pub async fn get_state(&self) -> VeilidAPIResult<VeilidState> {
        self.veilid.get_state().await
    }

    /// Is attached
    pub async fn is_attached(
        &self,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let state = self.veilid.get_state().await?;
        Ok(state.attachment.state.is_attached())
    }

	// yes, is_attached and is_network ready are the same thing, I'll probably combine them later, but for now I'm just experimenting.

    /// Is network ready
    pub async fn is_network_ready(
        &self,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let state = self.veilid.get_state().await?;

	Ok(matches!(
    	    state.attachment.state,
    	    AttachmentState::AttachedWeak
        	| AttachmentState::AttachedFair
        	| AttachmentState::AttachedGood
        	| AttachmentState::AttachedStrong
        	| AttachmentState::AttachedFull
        ))
    }
}

/// Create node
pub async fn create_node(
    namespace: String,
) -> Result<Arc<Node>, Box<dyn std::error::Error + Send + Sync>> {

    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|x| x.parent().map(|p| p.to_owned()))
        .unwrap_or_else(|| ".".into());

    let config = VeilidConfig {
        program_name: "Veilid Node".into(),
        namespace: namespace.clone(),

        protected_store: VeilidConfigProtectedStore {
            always_use_insecure_storage: false,

            directory: exe_dir
                .join(format!(".veilid/{}/protected_store", namespace))
                .to_string_lossy()
                .to_string(),

            ..Default::default()
        },

        table_store: VeilidConfigTableStore {
            directory: exe_dir
                .join(format!(".veilid/{}/table_store", namespace))
                .to_string_lossy()
                .to_string(),

            ..Default::default()
        },

        ..Default::default()
    };

    let handlers = Arc::new(NodeHandlers::new());

    let semaphore = Arc::new(Semaphore::new(50));

    let handlers_weak = Arc::downgrade(&handlers);

    let semaphore_clone = semaphore.clone();

    let callback = Arc::new(move |update: VeilidUpdate| {

        let Some(handlers) = handlers_weak.upgrade() else {
            return;
        };

        let semaphore = semaphore_clone.clone();

        tokio::spawn(async move {

            let permit = semaphore.acquire_owned().await;

            if permit.is_err() {
                return;
            }

            let _permit = permit.unwrap();

            handlers.handle_update(update).await;
        });
    });

    let veilid = veilid_core::api_startup(callback, config).await?;

    println!("[node] Attaching to network...");

    veilid.attach().await?;

    println!("[node] Waiting for network readiness...");

    wait_for_network(&veilid).await?;

    println!("[node] Network ready");

    let node = Arc::new(Node {
        veilid,
        handlers,
        update_semaphore: semaphore,
    });

    Ok(node)
}

/// Wait for network readiness
async fn wait_for_network(
    veilid: &VeilidAPI,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {

    loop {
        let state = veilid.get_state().await?;

        if state.attachment.public_internet_ready {
            return Ok(());
        }

        tokio::time::sleep(
            std::time::Duration::from_millis(500)
        ).await;
    }
}