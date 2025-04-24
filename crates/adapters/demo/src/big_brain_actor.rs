use nautilus_common::actor::Actor;
use nautilus_common::actor::registry::get_actor_unchecked;
use nautilus_common::messages::data::RequestData;
use nautilus_common::msgbus::handler::TypedMessageHandler;
use nautilus_common::msgbus::handler::{MessageHandler, ShareableMessageHandler};
use nautilus_common::msgbus::send;
use nautilus_common::msgbus::{register, register_request_handler};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::data::DataType;
use nautilus_model::identifiers::ClientId;
use std::any::Any;
use std::rc::Rc;
use ustr::Ustr;

pub struct BigBrainActor {
    pub pos_val: i32,
    pub neg_val: i32,
}

impl BigBrainActor {
    pub fn new() -> Self {
        Self {
            pos_val: 0,
            neg_val: 0,
        }
    }

    pub fn register_message_handlers() {
        let handler = TypedMessageHandler::from(negative_handler);
        let handler = ShareableMessageHandler::from(Rc::new(handler) as Rc<dyn MessageHandler>);
        register("negative_handler", handler);
    }
}

impl Actor for BigBrainActor {
    fn id(&self) -> Ustr {
        Ustr::from("big_brain_actor")
    }

    fn handle(&mut self, msg: &dyn Any) {
        todo!()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub fn negative_handler(msg: &i32) {
    let actor_id = Ustr::from("big_brain_actor");
    let big_brain_actor = get_actor_unchecked::<BigBrainActor>(&actor_id);
    big_brain_actor.neg_val = *msg;

    let correlation_id = UUID4::new();
    let handler = TypedMessageHandler::from(positive_handler);
    let handler = ShareableMessageHandler::from(Rc::new(handler) as Rc<dyn MessageHandler>);
    register_request_handler(&correlation_id, handler);

    let request = RequestData {
        client_id: ClientId::new("positive http stream"),
        data_type: DataType::new("positive_request", None),
        request_id: correlation_id,
        ts_init: UnixNanos::new(0),
        params: None,
    };

    if big_brain_actor.neg_val < -5 {
        send(&Ustr::from("positive_value_skip_request"), &request);
    } else {
        send(&Ustr::from("get_positive_value_request"), &request);
    }
}

pub fn positive_handler(msg: &i32) {
    let actor_id = Ustr::from("big_brain_actor");
    let big_brain_actor = get_actor_unchecked::<BigBrainActor>(&actor_id);
    big_brain_actor.pos_val = *msg;

    println!("Received positive value: {}", big_brain_actor.pos_val);

    if big_brain_actor.pos_val > 5 {
        send(&Ustr::from("subscriber_skip_command"), &());
    }

    if big_brain_actor.pos_val > 10 {
        send(&Ustr::from("subscriber_stop_command"), &());
    }
}
