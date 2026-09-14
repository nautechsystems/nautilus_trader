// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

//! Backend bindings for selected [`DataActor`](super::DataActor) facade methods.
//!
//! [`DataActorBinding`] supplies actor identity and clock access while author code uses
//! `self.actor_id()` and `self.clock()`. The blanket implementation for types implementing
//! [`DataActorNative`] delegates to the native core. Other bindings can provide these
//! operations without requiring the author to own a native core.
//!
//! The canonical facade methods document operation semantics, errors, and panics.

#![allow(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    reason = "errors and panics are documented on the canonical facade methods"
)]

use nautilus_model::identifiers::ActorId;

use super::DataActorNative;
use crate::clock::ClockApi;

#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "this DataActor method requires a DataActorBinding implementation"
)]
pub trait DataActorBinding {
    fn binding_actor_id(&self) -> ActorId;

    fn binding_clock(&self) -> ClockApi<'_>;
}

impl<T> DataActorBinding for T
where
    T: DataActorNative,
{
    fn binding_actor_id(&self) -> ActorId {
        self.core().actor_id()
    }

    fn binding_clock(&self) -> ClockApi<'_> {
        self.core().clock_api()
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::actor::DataActor;

    #[rstest]
    fn actor_behavior_requires_no_native_core() {
        assert_data_actor::<PortableActor>();
    }

    fn assert_data_actor<T: DataActor>() {
        let _: fn(&mut T) -> anyhow::Result<()> = T::on_start;
    }

    #[derive(Debug)]
    struct PortableActor;

    impl DataActor for PortableActor {}
}
