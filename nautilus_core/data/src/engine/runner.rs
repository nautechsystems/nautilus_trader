use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    rc::Rc,
    sync::Arc,
};

use nautilus_common::{
    actor::Actor,
    messages::data::{
        DataClientResponse,
        DataEngineRequest::{DataRequest, SubscriptionCommand},
        DataResponse,
    },
    msgbus::{self, MessageBus},
};
use nautilus_core::uuid::UUID4;
use nautilus_model::{data::Data, identifiers::ClientId};
use tokio::{
    sync::mpsc::{self, UnboundedReceiver, UnboundedSender},
    task::JoinHandle,
};

pub enum Runner {
    Live(LiveRunner),
}
pub struct LiveRunner {
    resp_tx: UnboundedSender<DataClientResponse>,
    resp_rx: UnboundedReceiver<DataClientResponse>,
}

impl LiveRunner {}

impl LiveRunner {
    pub fn run(msgbus: &MessageBus, resp_rx: &mut UnboundedReceiver<DataClientResponse>) {
        while let Some(resp) = resp_rx.blocking_recv() {
            match resp {
                DataClientResponse::DataResponse(data_resp) => {
                    // TODO: respond
                }
                DataClientResponse::Data(data) => {
                    // TODO: calculate endpoint from response
                    // TODO: handle logic for each data type before publishing on message bus
                    match data {
                        Data::Delta(data) => todo!(),
                        Data::Deltas(_) => todo!(),
                        Data::Depth10(_) => todo!(),
                        Data::Quote(_) => todo!(),
                        Data::Trade(_) => todo!(),
                        Data::Bar(_) => todo!(),
                    }
                    let endpoint = "TODO";
                    msgbus.publish(endpoint, &Box::new(data));
                }
            }
        }
    }
}
