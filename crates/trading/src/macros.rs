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

//! Convenience macros for implementing trading component boilerplate.

/// Implements `DataActorNative`, `StrategyNative`, and `Strategy` for a strategy type.
///
/// The struct must contain a field of type [`StrategyCore`](crate::strategy::StrategyCore).
/// By default the macro expects the field to be named `core`; pass a second argument
/// to use a different name.
/// The macro also adds an inherent `config()` method on the strategy type which
/// returns the user-supplied [`StrategyConfig`](crate::strategy::StrategyConfig).
///
/// An optional brace-delimited block adds extra methods to the generated `impl Strategy`.
/// Native runtime core access is generated through `StrategyNative`; normal strategy
/// logic should use `Strategy` facade methods.
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
        impl $ty {
            /// Returns the strategy configuration.
            #[allow(dead_code, unreachable_pub)]
            #[must_use]
            pub fn config(&self) -> &$crate::strategy::StrategyConfig {
                self.$field.config()
            }
        }

        impl $crate::_macro_reexports::DataActorNative for $ty {
            fn core(&self) -> &$crate::_macro_reexports::DataActorCore {
                $crate::_macro_reexports::DataActorNative::core(&self.$field)
            }

            fn core_mut(&mut self) -> &mut $crate::_macro_reexports::DataActorCore {
                $crate::_macro_reexports::DataActorNative::core_mut(&mut self.$field)
            }
        }

        impl $crate::strategy::StrategyNative for $ty {
            fn strategy_core(&self) -> &$crate::strategy::StrategyCore {
                &self.$field
            }

            fn strategy_core_mut(&mut self) -> &mut $crate::strategy::StrategyCore {
                &mut self.$field
            }
        }

        impl $crate::strategy::Strategy for $ty {
            $($extra)*
        }
    };
}

/// Implements `DataActorNative`, `ExecutionAlgorithmNative`, and `ExecutionAlgorithm` for an
/// execution algorithm type.
///
/// The struct must contain a field of type
/// [`ExecutionAlgorithmCore`](crate::algorithm::ExecutionAlgorithmCore). By default the macro
/// expects the field to be named `core`; pass a second argument to use a different name.
///
/// A brace-delimited block adds `on_order` and any extra methods to the generated
/// `impl ExecutionAlgorithm`. Native runtime core access is generated through
/// `ExecutionAlgorithmNative`; normal execution algorithm logic should use
/// `ExecutionAlgorithm` facade methods.
///
/// # Examples
///
/// ```ignore
/// use nautilus_trading::{algorithm::ExecutionAlgorithmCore, nautilus_execution_algorithm};
///
/// pub struct MyExecutionAlgorithm {
///     core: ExecutionAlgorithmCore,
///     // ...
/// }
///
/// nautilus_execution_algorithm!(MyExecutionAlgorithm, {
///     fn on_order(&mut self, order: OrderAny) -> anyhow::Result<()> {
///         // custom handling
///         Ok(())
///     }
/// });
/// ```
///
/// With a custom field name and hooks:
///
/// ```ignore
/// pub struct MyExecutionAlgorithm {
///     algorithm_core: ExecutionAlgorithmCore,
///     // ...
/// }
///
/// nautilus_execution_algorithm!(MyExecutionAlgorithm, algorithm_core, {
///     fn on_order(&mut self, order: OrderAny) -> anyhow::Result<()> {
///         // custom handling
///         Ok(())
///     }
/// });
/// ```
#[macro_export]
macro_rules! nautilus_execution_algorithm {
    ($ty:ty) => {
        compile_error!(
            "nautilus_execution_algorithm! requires an `on_order` implementation block"
        );
    };
    ($ty:ty, $field:ident) => {
        compile_error!(
            "nautilus_execution_algorithm! requires an `on_order` implementation block"
        );
    };
    ($ty:ty, { $($extra:item)* }) => {
        $crate::nautilus_execution_algorithm!($ty, core, { $($extra)* });
    };
    ($ty:ty, $field:ident, { $($extra:item)* }) => {
        impl $crate::_macro_reexports::DataActorNative for $ty {
            fn core(&self) -> &$crate::_macro_reexports::DataActorCore {
                $crate::_macro_reexports::DataActorNative::core(&self.$field)
            }

            fn core_mut(&mut self) -> &mut $crate::_macro_reexports::DataActorCore {
                $crate::_macro_reexports::DataActorNative::core_mut(&mut self.$field)
            }
        }

        impl $crate::algorithm::ExecutionAlgorithmNative for $ty {
            fn exec_algorithm_core(&self) -> &$crate::algorithm::ExecutionAlgorithmCore {
                &self.$field
            }

            fn exec_algorithm_core_mut(
                &mut self,
            ) -> &mut $crate::algorithm::ExecutionAlgorithmCore {
                &mut self.$field
            }
        }

        impl $crate::algorithm::ExecutionAlgorithm for $ty {
            $($extra)*
        }
    };
}
