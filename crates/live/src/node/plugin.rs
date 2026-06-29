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

//! Live-node plug-in support.
//!
//! The OSS live crate does not host dynamic plug-ins directly. Public
//! `nautilus-plugin` is the guest SDK; host-side loading, vtables, bridge
//! adapters, and server policy belong to the host-side plug-in integration.

#[derive(Debug, Default)]
pub(crate) struct NodePlugins;

impl NodePlugins {
    #[expect(
        clippy::unnecessary_wraps,
        clippy::unused_self,
        reason = "compatibility stub preserves the host-owned lifecycle contract"
    )]
    pub(crate) fn start_controllers(&self) -> anyhow::Result<()> {
        Ok(())
    }

    #[expect(
        clippy::unnecessary_wraps,
        clippy::unused_self,
        reason = "compatibility stub preserves the host-owned lifecycle contract"
    )]
    pub(crate) fn stop_controllers(&self) -> anyhow::Result<()> {
        Ok(())
    }
}
