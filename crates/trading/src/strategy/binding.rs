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

//! Backend bindings for selected [`Strategy`] facade methods.
//!
//! [`StrategyBinding`] connects calls such as `self.submit_order(...)` to their backing
//! without changing author syntax. The blanket implementation for types implementing
//! [`Strategy`] and [`StrategyNative`] delegates to native submission. Other bindings can
//! provide these operations without requiring the author to own a native core.
//!
//! The canonical facade methods document operation semantics, errors, and panics.

#![allow(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    reason = "errors and panics are documented on the canonical facade methods"
)]

use nautilus_core::Params;
use nautilus_model::{
    identifiers::{ClientId, PositionId},
    orders::OrderAny,
};

use super::{Strategy, StrategyNative, submit_order_native};

#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "this Strategy method requires a StrategyBinding implementation"
)]
pub trait StrategyBinding {
    fn binding_submit_order(
        &mut self,
        order: OrderAny,
        position_id: Option<PositionId>,
        client_id: Option<ClientId>,
        params: Option<Params>,
    ) -> anyhow::Result<()>;
}

impl<T> StrategyBinding for T
where
    T: Strategy + StrategyNative + ?Sized,
{
    fn binding_submit_order(
        &mut self,
        order: OrderAny,
        position_id: Option<PositionId>,
        client_id: Option<ClientId>,
        params: Option<Params>,
    ) -> anyhow::Result<()> {
        submit_order_native(self, &order, position_id, client_id, params)
    }
}

#[cfg(test)]
mod tests {
    use nautilus_common::{
        actor::{DataActor, binding::DataActorBinding},
        clock::ClockApi,
    };
    use nautilus_model::identifiers::ActorId;
    use rstest::rstest;

    use super::*;
    use crate::{ExecutionAlgorithm, nautilus_strategy, strategy::StrategyCore};

    #[rstest]
    fn canonical_facades_require_only_binding_traits() {
        assert_binding_signatures::<NativeStrategy>();
    }

    fn assert_binding_signatures<T: Strategy + DataActorBinding + StrategyBinding>() {
        let _: fn(&T) -> ActorId = T::actor_id;
        let _: fn(&T) -> ActorId = T::binding_actor_id;
        let _: fn(&T) -> ClockApi<'_> = T::clock;
        let _: fn(&T) -> ClockApi<'_> = T::binding_clock;
        let _: SubmitOrder<T> = T::submit_order;
        let _: SubmitOrder<T> = T::binding_submit_order;
    }

    type SubmitOrder<T> = fn(
        &mut T,
        OrderAny,
        Option<PositionId>,
        Option<ClientId>,
        Option<Params>,
    ) -> anyhow::Result<()>;

    #[derive(Debug)]
    struct NativeStrategy {
        core: StrategyCore,
    }

    impl DataActor for NativeStrategy {}

    nautilus_strategy!(NativeStrategy);

    #[rstest]
    fn behavioral_roles_do_not_require_native_cores() {
        assert_strategy::<PortableStrategy>();
        assert_execution_algorithm::<PortableExecutionAlgorithm>();
    }

    fn assert_data_actor<T: DataActor>() {
        let _: fn(&mut T) -> anyhow::Result<()> = T::on_start;
    }

    fn assert_strategy<T: Strategy>() {
        assert_data_actor::<T>();
    }

    fn assert_execution_algorithm<T: ExecutionAlgorithm>() {
        assert_data_actor::<T>();
        let _: fn(&mut T, OrderAny) -> anyhow::Result<()> = T::on_order;
    }

    #[derive(Debug)]
    struct PortableStrategy;

    impl DataActor for PortableStrategy {}

    impl Strategy for PortableStrategy {}

    #[derive(Debug)]
    struct PortableExecutionAlgorithm;

    impl DataActor for PortableExecutionAlgorithm {}

    impl ExecutionAlgorithm for PortableExecutionAlgorithm {
        fn on_order(&mut self, _order: OrderAny) -> anyhow::Result<()> {
            Ok(())
        }
    }
}
