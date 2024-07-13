// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

use std::collections::HashMap;

use nautilus_common::{
    actor::Actor,
    messages::data::{DataRequest, DataResponse},
};
use nautilus_core::uuid::UUID4;
use nautilus_model::identifiers::ClientId;

struct SyncDataEngine;
struct LiveDataEngine;

// TODO: Draft client
pub trait DataClient: Sync + Send {
    fn handle(&self, req: DataRequest) -> DataResponse;
    fn id(&self) -> ClientId;
}

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
