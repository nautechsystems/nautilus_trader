use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    rc::Rc,
};

use nautilus_common::{
    actor::Actor,
    messages::data::{
        DataEngineRequest::{DataRequest, SubscriptionCommand},
        DataResponse,
    },
};
use nautilus_core::uuid::UUID4;
use nautilus_model::identifiers::ClientId;

use crate::client::DataClientAdaptor;

use super::DataRequestQueue;

pub struct LiveRunner;

impl LiveRunner {
    pub fn run(
        req_queue: DataRequestQueue,
        clients: &HashMap<ClientId, DataClientAdaptor>,
        actors: &HashMap<UUID4, Box<dyn Actor>>,
    ) {
        let (resp_tx, resp_rx) = tokio::sync::mpsc::unbounded_channel::<DataResponse>();

        while let Some(req) = req_queue.next() {
            match req {
                DataRequest(req) => {
                    let client = clients.get(&req.client_id);
                    // TODO: restructure data request and response fields
                    // so that appropriate field is used to lookup actor
                    let actor = actors.get(&req.correlation_id);
                    match (client, actor) {
                        (Some(client), Some(actor)) => {
                            let resp = client.request(req);
                            actor.handle(resp)
                        }
                        _ => {
                            // TODO: log error
                        }
                    }
                }
                SubscriptionCommand(req) => todo!(),
            }
        }
    }
}

pub struct BacktestRunner {
    resp_queue: Rc<RefCell<VecDeque<DataResponse>>>,
}

impl BacktestRunner {
    pub fn run(
        req_queue: DataRequestQueue,
        clients: &HashMap<ClientId, DataClientAdaptor>,
        actors: &HashMap<UUID4, Box<dyn Actor>>,
    ) {
        while let Some(req) = req_queue.next() {
            match req {
                DataRequest(req) => {
                    let client = clients.get(&req.client_id);
                    // TODO: restructure data request and response fields
                    // so that appropriate field is used to lookup actor
                    let actor = actors.get(&req.correlation_id);
                    match (client, actor) {
                        (Some(client), Some(actor)) => {
                            let resp = client.request(req);
                            actor.handle(resp)
                        }
                        _ => {
                            // TODO: log error
                        }
                    }
                }
                SubscriptionCommand(req) => todo!(),
            }
        }
    }
}
