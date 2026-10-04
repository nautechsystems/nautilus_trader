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

use nautilus_common::{
    actor::{self, CallbackDispatchError},
    live::dst,
};

const CALLBACK_DRAIN_BUDGET: usize = 64;

pub(crate) async fn drain_callbacks() -> Result<bool, CallbackDispatchError> {
    let pending = actor::drain_callbacks(CALLBACK_DRAIN_BUDGET)?;
    if pending {
        dst::task::yield_now().await;
    }

    Ok(pending)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn latch_callback_failure() {
        struct DrainOnDrop;

        impl Drop for DrainOnDrop {
            fn drop(&mut self) {
                assert_eq!(actor::drain_callbacks(1), Ok(false));
            }
        }

        let result = std::panic::catch_unwind(|| {
            let _drain = DrainOnDrop;
            panic!("simulated callback unwind");
        });

        assert!(result.is_err());
        assert_eq!(
            actor::callback_failure(),
            Some(CallbackDispatchError::DeliveryUnwound)
        );
    }
}
