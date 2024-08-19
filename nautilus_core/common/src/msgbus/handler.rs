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

use std::{any::Any, rc::Rc};

use nautilus_model::data::Data;
use ustr::Ustr;

use crate::messages::data::DataResponse;

pub trait MessageHandler: Any {
    fn id(&self) -> Ustr;
    fn handle(&self, message: &dyn Any);
    fn handle_response(&self, resp: DataResponse);
    fn handle_data(&self, data: Data);
    fn as_any(&self) -> &dyn Any;
}

#[derive(Clone)]
#[repr(transparent)]
pub struct ShareableMessageHandler(pub Rc<dyn MessageHandler>);

impl From<Rc<dyn MessageHandler>> for ShareableMessageHandler {
    fn from(value: Rc<dyn MessageHandler>) -> Self {
        Self(value)
    }
}

// SAFETY: Message handlers cannot be sent across thread boundaries
unsafe impl Send for ShareableMessageHandler {}
