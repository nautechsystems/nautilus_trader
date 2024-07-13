use std::collections::HashMap;

use nautilus_core::uuid::UUID4;
use nautilus_model::identifiers::ClientId;

pub struct DataRequest {
    actor_id: UUID4,
    req_id: UUID4,
    client_id: ClientId,
}

pub struct DataResponse {
    actor_id: UUID4,
    req_id: UUID4,
    client_id: ClientId,
}

pub trait DataClient: Sync + Send {
    fn handle(&self, req: DataRequest) -> DataResponse;
    fn id(&self) -> ClientId;
}

pub trait Actor {
    fn handle(&self, resp: DataResponse);
    fn id(&self) -> UUID4;
}

struct SyncDataEngine;
struct LiveDataEngine;

pub struct DataEngineConfig {
    clients: HashMap<ClientId, Box<dyn DataClient>>,
    actors: HashMap<UUID4, Box<dyn Actor>>,
    req_tx: tokio::sync::mpsc::UnboundedSender<DataRequest>,
    req_rx: tokio::sync::mpsc::UnboundedReceiver<DataRequest>,
    resp_tx: tokio::sync::mpsc::Sender<DataResponse>,
    resp_rx: tokio::sync::mpsc::Receiver<DataResponse>,
}

impl DataEngineConfig {
    pub fn new() -> Self {
        let (req_tx, req_rx) = tokio::sync::mpsc::unbounded_channel::<DataRequest>();
        let (resp_tx, resp_rx) = tokio::sync::mpsc::channel::<DataResponse>(10);
        Self {
            clients: HashMap::new(),
            actors: HashMap::new(),
            req_tx,
            req_rx,
            resp_tx,
            resp_rx,
        }
    }

    pub fn add_client(&mut self, client: Box<dyn DataClient>) {
        self.clients.insert(client.id(), client);
    }

    pub fn add_actor(&mut self, actor: Box<dyn Actor>) {
        self.actors.insert(actor.id(), actor);
    }
}

impl Default for DataEngineConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl SyncDataEngine {
    fn start(config: DataEngineConfig) {
        let DataEngineConfig {
            clients,
            actors,
            req_tx: _,
            req_rx,
            resp_tx,
            mut resp_rx,
        } = config;

        // TODO: Run in a separate thread
        let request_handler_task = request_handler_task(req_rx, resp_tx, clients);

        // TODO: consider which tokio runtime to use for blocking
        while let Some(resp) = resp_rx.blocking_recv() {
            if let Some(actor) = actors.get(&resp.actor_id) {
                actor.handle(resp);
            }
        }

        request_handler_task.abort();
    }
}

impl LiveDataEngine {
    fn start(config: DataEngineConfig) {
        let DataEngineConfig {
            clients,
            actors,
            req_tx: _,
            req_rx,
            resp_tx,
            mut resp_rx,
        } = config;

        // TODO: Run in a separate thread
        let request_handler_task = request_handler_task(req_rx, resp_tx, clients);

        // TODO: consider which tokio runtime to use for blocking
        while let Some(resp) = resp_rx.blocking_recv() {
            if let Some(actor) = actors.get(&resp.actor_id) {
                actor.handle(resp);
            }
        }

        request_handler_task.abort();
    }
}

fn request_handler_task(
    mut req_rx: tokio::sync::mpsc::UnboundedReceiver<DataRequest>,
    resp_tx: tokio::sync::mpsc::Sender<DataResponse>,
    clients: HashMap<ClientId, Box<dyn DataClient>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(req) = req_rx.recv().await {
            if let Some(client) = clients.get(&req.client_id) {
                let resp = client.handle(req);
                // TODO add logging
                match resp_tx.send(resp).await {
                    Ok(_) => todo!(),
                    Err(_) => todo!(),
                };
            }
        }
    })
}
