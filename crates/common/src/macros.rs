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

/// Implements `Deref<Target = DataActorCore>` and `DerefMut` for an actor type.
///
/// The struct must contain a field that dereferences to
/// [`DataActorCore`](crate::actor::DataActorCore), either directly or through
/// an intermediate type (e.g. `ExecutionAlgorithmCore`).
/// By default the macro expects the field to be named `core`; pass a second argument
/// to use a different name.
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
        impl ::std::ops::Deref for $ty {
            type Target = $crate::actor::DataActorCore;

            fn deref(&self) -> &Self::Target {
                &self.$field
            }
        }

        impl ::std::ops::DerefMut for $ty {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.$field
            }
        }
    };
}
