use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    rc::Rc,
};

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
    sync_data_client: SyncDataEngineClient,
    req_tx: tokio::sync::mpsc::UnboundedSender<DataRequest>,
    req_rx: tokio::sync::mpsc::UnboundedReceiver<DataRequest>,
    resp_tx: tokio::sync::mpsc::Sender<DataResponse>,
    resp_rx: tokio::sync::mpsc::Receiver<DataResponse>,
    live: bool,
}

#[derive(Clone)]
struct SyncDataEngineClient {
    req_queue: Rc<RefCell<VecDeque<DataRequest>>>,
}

#[derive(Clone)]
struct AsyncDataEngineClient {
    req_tx: tokio::sync::mpsc::UnboundedSender<DataRequest>,
}

pub trait DataEngineClient {
    fn send(&self, req: DataRequest);
}

impl DataEngineClient for SyncDataEngineClient {
    fn send(&self, req: DataRequest) {
        self.req_queue.borrow_mut().push_back(req);
    }
}

impl DataEngineConfig {
    pub fn new(live: bool) -> Self {
        let (req_tx, req_rx) = tokio::sync::mpsc::unbounded_channel::<DataRequest>();
        let (resp_tx, resp_rx) = tokio::sync::mpsc::channel::<DataResponse>(10);
        let req_queue = Rc::new(RefCell::new(VecDeque::new()));
        Self {
            clients: HashMap::new(),
            actors: HashMap::new(),
            sync_data_client: SyncDataEngineClient { req_queue },
            live,
        }
    }

    pub fn add_client(&mut self, client: Box<dyn DataClient>) {
        self.clients.insert(client.id(), client);
    }

    pub fn add_actor(&mut self, actor: Box<dyn Actor>) {
        self.actors.insert(actor.id(), actor);
    }

    pub fn get_data_engine_client(&self) -> Rc<dyn DataEngineClient> {
        if self.live {
        } else {
            self.sync_data_client
        }
    }
}

impl Default for DataEngineConfig {
    fn default() -> Self {
        Self::new(false)
    }
}

impl SyncDataEngine {
    fn start(config: DataEngineConfig) {
        let DataEngineConfig {
            clients,
            actors,
            req_queue,
            live,
        } = config;

        // TODO: consider which tokio runtime to use for blocking
        while let Some(req) = req_queue.borrow_mut().pop_front() {
            let client = clients.get(&req.client_id);
            let actor = actors.get(&req.actor_id);
            match (client, actor) {
                (Some(client), Some(actor)) => {
                    let resp = client.handle(req);
                    actor.handle(resp)
                }
                _ => {
                    // TODO: log error
                }
            }
        }
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
