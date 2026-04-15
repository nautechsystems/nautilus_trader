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

//! Shared IB API client connection per (host, port, client_id).
//!
//! Data, execution, and historical clients use a single TCP connection per logical
//! connection to avoid client ID conflicts and redundant connections (parity with
//! Python's get_cached_ib_client).

use std::{
    collections::HashMap,
    fmt::Debug,
    ops::Deref,
    sync::{Arc, LazyLock, Mutex},
    time::Duration,
};

use anyhow::Context;
use ibapi::client::Client;

/// Key for the connection registry: (host, port, client_id).
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ConnectionKey(String, u16, i32);

/// Registry entry: shared client and its ref count.
type RegistryMap = HashMap<ConnectionKey, (Arc<Client>, u32)>;

/// Global registry: one shared client per (host, port, client_id) with ref count.
static REGISTRY: LazyLock<Arc<Mutex<RegistryMap>>> =
    LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

/// Handle to a shared IB client; when dropped, ref count is decremented and the
/// connection is removed from the registry when the count reaches zero.
pub struct SharedClientHandle {
    client: Arc<Client>,
    registry: Arc<Mutex<RegistryMap>>,
    key: ConnectionKey,
}

impl Debug for SharedClientHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(SharedClientHandle))
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

impl SharedClientHandle {
    fn new(client: Arc<Client>, registry: Arc<Mutex<RegistryMap>>, key: ConnectionKey) -> Self {
        Self {
            client,
            registry,
            key,
        }
    }

    /// Returns a reference to the underlying `Arc<Client>` for call sites that need it.
    pub fn as_arc(&self) -> &Arc<Client> {
        &self.client
    }
}

impl Deref for SharedClientHandle {
    type Target = Client;

    fn deref(&self) -> &Self::Target {
        self.client.as_ref()
    }
}

impl Drop for SharedClientHandle {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.registry.lock()
            && let Some((_, ref_count)) = guard.get_mut(&self.key)
        {
            *ref_count = ref_count.saturating_sub(1);
            if *ref_count == 0 {
                guard.remove(&self.key);
                tracing::debug!(
                    "Shared IB client removed from registry (host={}, port={}, client_id={})",
                    self.key.0,
                    self.key.1,
                    self.key.2
                );
            }
        }
    }
}

/// Returns a handle to the shared IB client for the given (host, port, client_id).
/// If a connection already exists, its ref count is incremented and the same client
/// is returned. Otherwise a new connection is established and registered.
///
/// # Errors
///
/// Returns an error if connecting to IB Gateway/TWS fails.
pub async fn get_or_connect(
    host: &str,
    port: u16,
    client_id: i32,
    connection_timeout_secs: u64,
) -> anyhow::Result<SharedClientHandle> {
    let key = ConnectionKey(host.to_string(), port, client_id);
    let registry = Arc::clone(&REGISTRY);

    log::debug!(
        "Acquiring shared IB client (host={}, port={}, client_id={}, timeout_secs={})",
        host,
        port,
        client_id,
        connection_timeout_secs
    );

    let (reuse_client, ref_count_val) = {
        let mut guard = registry
            .lock()
            .map_err(|e| anyhow::anyhow!("Registry mutex poisoned: {e}"))?;

        if let Some((client, ref_count)) = guard.get_mut(&key) {
            *ref_count += 1;
            let ref_count_val = *ref_count;
            let client = Arc::clone(client);
            (Some(client), ref_count_val)
        } else {
            (None, 0)
        }
    };

    if let Some(client) = reuse_client {
        log::debug!(
            "Reusing shared IB client (host={}, port={}, client_id={}, ref_count={})",
            host,
            port,
            client_id,
            ref_count_val
        );
        return Ok(SharedClientHandle::new(client, registry, key));
    }

    let address = format!("{host}:{port}");
    let connect_timeout = Duration::from_secs(connection_timeout_secs);
    log::debug!(
        "No shared IB client found, establishing new connection to {} with timeout {:?}",
        address,
        connect_timeout
    );
    let client = tokio::time::timeout(connect_timeout, Client::connect(&address, client_id))
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "Timed out connecting to IB Gateway/TWS after {}s",
                connection_timeout_secs
            )
        })?
        .context("Failed to connect to IB Gateway/TWS")?;
    let client = Arc::new(client);

    {
        let mut guard = registry
            .lock()
            .map_err(|e| anyhow::anyhow!("Registry mutex poisoned: {e}"))?;
        log::debug!(
            "Registering shared IB client in registry (host={}, port={}, client_id={})",
            host,
            port,
            client_id
        );
        guard.insert(key.clone(), (Arc::clone(&client), 1));
    }

    tracing::info!(
        "Registered new shared IB client (host={}, port={}, client_id={})",
        host,
        port,
        client_id
    );

    Ok(SharedClientHandle::new(client, registry, key))
}
