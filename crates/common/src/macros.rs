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

//! Convenience macros for implementing actor boilerplate.

/// Wires an actor type's core field into the native runtime contract.
///
/// The struct must contain a field that provides a
/// [`DataActorCore`](crate::actor::DataActorCore) reference, either directly or
/// by deref coercion through an intermediate core type (e.g. `ExecutionAlgorithmCore`).
/// By default the macro expects the field to be named `core`; pass a second argument
/// to use a different name.
///
/// The generated native access implementation is runtime wiring. Normal actor code
/// should use [`DataActor`](crate::actor::DataActor) facade methods such as
/// `clock()`, `cache()`, and the subscription methods.
///
/// # Examples
///
/// ```ignore
/// use nautilus_common::{nautilus_actor, actor::DataActorCore};
///
/// pub struct MyActor {
///     core: DataActorCore,
///     // ...
/// }
///
/// nautilus_actor!(MyActor);
/// ```
///
/// With a custom field name:
///
/// ```ignore
/// pub struct MyActor {
///     actor_core: DataActorCore,
///     // ...
/// }
///
/// nautilus_actor!(MyActor, actor_core);
/// ```
#[macro_export]
macro_rules! nautilus_actor {
    ($ty:ty) => {
        $crate::nautilus_actor!($ty, core);
    };
    ($ty:ty, $field:ident) => {
        impl $crate::actor::DataActorNative for $ty {
            fn core(&self) -> &$crate::actor::DataActorCore {
                &self.$field
            }

            fn core_mut(&mut self) -> &mut $crate::actor::DataActorCore {
                &mut self.$field
            }
        }
    };
}
