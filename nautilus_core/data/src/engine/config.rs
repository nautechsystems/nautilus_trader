use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    rc::Rc,
};

use nautilus_core::uuid::UUID4;
use nautilus_model::identifiers::ClientId;

// TODO: temporarily use this trait until actual data client trait is fixed
pub trait DataClient: Sync + Send {
    fn handle(&self, req: DataRequest) -> DataResponse;
    fn client_id(&self) -> ClientId;
}

pub trait Actor {
    fn handle(&self, resp: DataResponse);
    fn id(&self) -> UUID4;
}

pub struct DataRequest {
    pub actor_id: UUID4,
    pub req_id: UUID4,
    pub client_id: ClientId,
}

pub struct DataResponse {
    pub actor_id: UUID4,
    pub req_id: UUID4,
    pub client_id: ClientId,
}

pub struct DataEngineConfig {
    pub clients: HashMap<ClientId, Box<dyn DataClient>>,
    pub actors: HashMap<UUID4, Box<dyn Actor>>,
    pub req_queue: Rc<RefCell<VecDeque<DataRequest>>>,
    pub req_tx: tokio::sync::mpsc::UnboundedSender<DataRequest>,
    pub req_rx: tokio::sync::mpsc::UnboundedReceiver<DataRequest>,
    pub live: bool,
}

impl Default for DataEngineConfig {
    fn default() -> Self {
        Self::new(false)
    }
}

impl DataEngineConfig {
    pub fn new(live: bool) -> Self {
        let (req_tx, req_rx) = tokio::sync::mpsc::unbounded_channel::<DataRequest>();
        let req_queue = Rc::new(RefCell::new(VecDeque::new()));
        Self {
            clients: HashMap::new(),
            actors: HashMap::new(),
            req_queue,
            req_tx,
            req_rx,
            live,
        }
    }

    pub fn add_client(&mut self, client: Box<dyn DataClient>) {
        self.clients.insert(client.client_id(), client);
    }

    pub fn add_actor(&mut self, actor: Box<dyn Actor>) {
        self.actors.insert(actor.id(), actor);
    }

    /// Get a clone of the appropriate data engine client
    ///
    /// This can be called by actors to get an interface to the data engine.
    /// The actor can store it and send data requests to the [`DataEngine`]
    /// through it.
    pub fn get_data_engine_client(&self) -> Box<dyn DataEngineClient> {
        if self.live {
            Box::new(LiveDataEngineClient {
                req_tx: self.req_tx.clone(),
            })
        } else {
            Box::new(BacktestDataEngineClient {
                req_queue: self.req_queue.clone(),
            })
        }
    }
}

#[derive(Clone)]
struct BacktestDataEngineClient {
    req_queue: Rc<RefCell<VecDeque<DataRequest>>>,
}

#[derive(Clone)]
struct LiveDataEngineClient {
    req_tx: tokio::sync::mpsc::UnboundedSender<DataRequest>,
}

/// Data engine client sends data requests to the [`DataEngine`]
pub trait DataEngineClient {
    fn send(&self, req: DataRequest);
}

impl DataEngineClient for BacktestDataEngineClient {
    fn send(&self, req: DataRequest) {
        self.req_queue.borrow_mut().push_back(req);
    }
}

impl DataEngineClient for LiveDataEngineClient {
    fn send(&self, req: DataRequest) {
        if let Err(e) = self.req_tx.send(req) {
            // TODO log error
        }
    }
}
