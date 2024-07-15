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

use nautilus_model::identifiers::ClientId;

use super::config::{DataClient, DataEngineConfig, DataRequest, DataResponse};

struct LiveDataEngine;

impl LiveDataEngine {
    fn start(config: DataEngineConfig) {
        let DataEngineConfig {
            clients,
            actors,
            req_tx,
            req_rx,
            ..
        } = config;

        let (resp_tx, mut resp_rx) = tokio::sync::mpsc::channel::<DataResponse>(10);
        // TODO: Run in a separate thread
        let request_handler_task = request_handler_task(req_rx, resp_tx, clients);

        // TODO: consider which tokio runtime to use for blocking
        while let Some(resp) = resp_rx.blocking_recv() {
            if let Some(actor) = actors.get(&resp.req_id) {
                actor.handle(resp);
            }
        }

        request_handler_task.abort();
    }
}

fn request_handler_task(
    mut req_rx: tokio::sync::mpsc::UnboundedReceiver<DataRequest>,
    resp_tx: tokio::sync::mpsc::Sender<DataResponse>, // TODO: Draft
    clients: HashMap<ClientId, Box<dyn DataClient>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(req) = req_rx.recv().await {
            if let Some(client) = clients.get(&req.client_id) {
                let resp = client.handle(req);
                // TODO add logging
                match resp_tx.send(resp).await {
                    Ok(()) => todo!(),
                    Err(_) => todo!(),
                };
            }
        }
    })
}
