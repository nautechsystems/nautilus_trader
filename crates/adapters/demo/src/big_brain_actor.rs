// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{any::Any, rc::Rc};

use nautilus_common::{
    actor::{Actor, registry::get_actor_unchecked},
    messages::data::{
        DataCommand, RequestCommand, RequestCustomData, SubscribeCommand, SubscribeCustomData,
        UnsubscribeCommand, UnsubscribeCustomData,
    },
    msgbus::{
        handler::{MessageHandler, ShareableMessageHandler, TypedMessageHandler},
        register, register_response_handler, send,
    },
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{data::DataType, identifiers::ClientId};
use ustr::Ustr;

/// Big brain actor receives positive and negative streams of numbers
///
/// The negative drives the positive stream and the postive after reaching
/// 10 issues a stop command. The actor should ideally behave like this
///
/// -1 -> get request
/// 1
/// -2 -> get request
/// 2
/// -3 -> skip request
/// 7 -> skip command
/// -8 -> get request
/// 8 -> stop command
pub struct BigBrainActor {
    pub pos_val: i32,
    pub neg_val: i32,
}

impl Default for BigBrainActor {
    fn default() -> Self {
        Self::new()
    }
}

impl BigBrainActor {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pos_val: 0,
            neg_val: 0,
        }
    }

    pub fn register_message_handlers() {
        let handler = TypedMessageHandler::from(negative_handler);
        let handler = ShareableMessageHandler::from(Rc::new(handler) as Rc<dyn MessageHandler>);
        let endpoint = "negative_stream".into();
        register(endpoint, handler);
    }
}

impl Actor for BigBrainActor {
    fn id(&self) -> Ustr {
        Ustr::from("big_brain_actor")
    }

    fn handle(&mut self, _msg: &dyn Any) {
        todo!()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Negative integer stream handler
///
/// It prints each positive number it receives. For each negative number
/// it makes requests a positive number. When negative number is equal to -3
/// it issues a skipped positive number request instead.
pub fn negative_handler(msg: &i32) {
    let actor_id = Ustr::from("big_brain_actor");
    let big_brain_actor = get_actor_unchecked::<BigBrainActor>(&actor_id);
    big_brain_actor.neg_val = *msg;

    println!("Received negative value: {}", big_brain_actor.neg_val);

    let correlation_id = UUID4::new();
    let handler = TypedMessageHandler::from(positive_handler);
    let handler = ShareableMessageHandler::from(Rc::new(handler) as Rc<dyn MessageHandler>);
    register_response_handler(&correlation_id, handler);

    let data_type = if big_brain_actor.neg_val == -3 {
        DataType::new("skip", None)
    } else {
        DataType::new("get", None)
    };

    let request = RequestCustomData {
        client_id: ClientId::new("mock_data_client"),
        data_type,
        request_id: correlation_id,
        ts_init: UnixNanos::new(0),
        params: None,
    };
    let cmd = DataCommand::Request(RequestCommand::Data(request));

    send("data_engine".into(), &cmd);
}

/// Positive integer stream handler
///
/// It prints each positive number it receives. When the positive value
/// exceeds 3, it issues a skip command for the negative stream. When it exceeds
/// 8 it issues a stop command for the negative stream
pub fn positive_handler(msg: &i32) {
    let actor_id = Ustr::from("big_brain_actor");
    let big_brain_actor = get_actor_unchecked::<BigBrainActor>(&actor_id);
    big_brain_actor.pos_val = *msg;

    println!("Received positive value: {}", big_brain_actor.pos_val);

    let data_type = DataType::new("blah", None);

    if big_brain_actor.pos_val == 3 {
        let data = SubscribeCustomData::new(
            Some(ClientId::new("mock_data_client")),
            None,
            data_type.clone(),
            UUID4::new(),
            UnixNanos::new(0),
            None,
        );
        let cmd = DataCommand::Subscribe(SubscribeCommand::Data(data));
        send("data_engine".into(), &cmd);
    }

    if big_brain_actor.pos_val > 8 {
        let data = UnsubscribeCustomData::new(
            Some(ClientId::new("mock_data_client")),
            None,
            data_type,
            UUID4::new(),
            UnixNanos::new(0),
            None,
        );
        let cmd = DataCommand::Unsubscribe(UnsubscribeCommand::Data(data));
        send("data_engine".into(), &cmd);
    }
}
