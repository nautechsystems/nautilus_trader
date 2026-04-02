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

//! Convenience macros for implementing strategy boilerplate.

/// Implements `Deref<Target = DataActorCore>`, `DerefMut`, and `Strategy` for a strategy type.
///
/// The struct must contain a field of type [`StrategyCore`](crate::strategy::StrategyCore).
/// By default the macro expects the field to be named `core`; pass a second argument
/// to use a different name.
///
/// An optional brace-delimited block adds extra methods to the generated `impl Strategy`.
/// Do not redefine `core` or `core_mut` inside the block; they are already generated
/// by the macro and duplicates will cause a compile error.
///
/// # Examples
///
/// ```ignore
/// use nautilus_trading::{nautilus_strategy, strategy::StrategyCore};
///
/// pub struct MyStrategy {
///     core: StrategyCore,
///     // ...
/// }
///
/// // Simple form
/// nautilus_strategy!(MyStrategy);
/// ```
///
/// With Strategy hook overrides:
///
/// ```ignore
/// nautilus_strategy!(MyStrategy, {
///     fn on_order_rejected(&mut self, event: OrderRejected) {
///         // custom handling
///     }
/// });
/// ```
///
/// With a custom field name and hooks:
///
/// ```ignore
/// pub struct MyStrategy {
///     strat_core: StrategyCore,
///     // ...
/// }
///
/// nautilus_strategy!(MyStrategy, strat_core, {
///     fn external_order_claims(&self) -> Option<Vec<InstrumentId>> {
///         None
///     }
/// });
/// ```
#[macro_export]
macro_rules! nautilus_strategy {
    ($ty:ty) => {
        $crate::nautilus_strategy!($ty, core, {});
    };
    ($ty:ty, $field:ident) => {
        $crate::nautilus_strategy!($ty, $field, {});
    };
    ($ty:ty, { $($extra:item)* }) => {
        $crate::nautilus_strategy!($ty, core, { $($extra)* });
    };
    ($ty:ty, $field:ident, { $($extra:item)* }) => {
        impl ::std::ops::Deref for $ty {
            type Target = $crate::_macro_reexports::DataActorCore;

            fn deref(&self) -> &Self::Target {
                &self.$field
            }
        }

        impl ::std::ops::DerefMut for $ty {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.$field
            }
        }

        impl $crate::strategy::Strategy for $ty {
            fn core(&self) -> &$crate::strategy::StrategyCore {
                &self.$field
            }

            fn core_mut(&mut self) -> &mut $crate::strategy::StrategyCore {
                &mut self.$field
            }

            $($extra)*
        }
    };
}
