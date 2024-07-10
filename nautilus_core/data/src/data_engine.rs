use std::{collections::HashMap, rc::Rc, sync::Arc};

use nautilus_core::uuid::UUID4;
use nautilus_model::identifiers::ClientId;

struct DataRequest {
    client_id: ClientId,
}

struct DataResponse {
    correlation_id: UUID4,
}

trait DataClient: Sync + Send {
    fn get_response(&self, req: DataRequest) -> DataResponse;
}

trait Actor {
    fn handle_response(&self, resp: DataResponse);
    fn id(&self) -> UUID4;
}

struct DataEngine {
    data_client_task: tokio::task::JoinHandle<()>,
    req_tx: std::sync::mpsc::Sender<DataRequest>,
}

impl DataEngine {
    fn new() -> Self {
        let (req_tx, req_rx) = std::sync::mpsc::channel::<DataRequest>();
        let (resp_tx, resp_rx) = std::sync::mpsc::channel::<DataResponse>();
        let client_mapping: HashMap<ClientId, Arc<dyn DataClient>> = HashMap::new();
        let actor_mapping: HashMap<UUID4, Rc<dyn Actor>> = HashMap::new();

        // TODO: Run in a separate thread
        let data_client_task = tokio::task::spawn(async move {
            while let Ok(req) = req_rx.recv() {
                // TODO: Move each client request handling to a separate task
                if let Some(client) = client_mapping.get(&req.client_id) {
                    let resp = client.get_response(req);
                    resp_tx.send(resp).unwrap(); // TODO
                }
            }
        });

        while let Ok(resp) = resp_rx.recv() {
            if let Some(actor) = actor_mapping.get(&resp.correlation_id) {
                actor.handle_response(resp);
            }
        }

        Self {
            data_client_task,
            req_tx,
        }
    }

    pub fn send_request(&self, req: DataRequest) {
        self.req_tx.send(req).unwrap();
    }
}
