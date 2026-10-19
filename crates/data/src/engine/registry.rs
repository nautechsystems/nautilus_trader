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

use super::{
    AHashSet, ClientId, DataClientAdapter, DataEngine, FAILED, Venue, check_key_in_map,
    check_key_not_in_map, check_predicate_true,
};

impl DataEngine {
    #[cfg(feature = "defi")]
    #[must_use]
    pub(crate) fn is_external_client(&self, client_id: ClientId) -> bool {
        self.external_clients.contains(&client_id)
    }

    /// Registers the `client` with the engine with an optional venue `routing`.
    ///
    ///
    /// # Panics
    ///
    /// Panics if a client with the same client ID has already been registered.
    pub fn register_client(&mut self, client: DataClientAdapter, routing: Option<Venue>) {
        let client_id = client.client_id();

        check_key_not_in_map(&client_id, &self.clients, "client_id", "clients").expect(FAILED);

        if let Some(routing) = routing {
            self.routing_map.insert(routing, client_id);
            log::debug!("Set client {client_id} routing for {routing}");
        }

        if client.venue.is_none() && self.default_client_id.is_none() {
            self.default_client_id = Some(client_id);
            log::debug!("Registered client {client_id} for default routing");
        }

        self.clients.insert(client_id, client);
        log::debug!("Registered client {client_id}");
    }

    /// Deregisters the client for the `client_id`.
    ///
    /// # Panics
    ///
    /// Panics if the client ID has not been registered.
    pub fn deregister_client(&mut self, client_id: &ClientId) {
        check_key_in_map(client_id, &self.clients, "client_id", "clients").expect(FAILED);

        if self.default_client_id.as_ref() == Some(client_id) {
            self.default_client_id = None;
        }

        self.clients.shift_remove(client_id);
        log::info!("Deregistered client {client_id}");
    }

    /// Registers the data `client` with the engine as the default routing client.
    ///
    /// When a specific venue routing cannot be found, this client will receive messages.
    ///
    /// # Warnings
    ///
    /// Any existing default routing client will be overwritten.
    ///
    /// # Panics
    ///
    /// Panics if a default client has already been registered.
    pub fn register_default_client(&mut self, client: DataClientAdapter) {
        check_predicate_true(
            self.default_client_id.is_none(),
            "default client already registered",
        )
        .expect(FAILED);

        let client_id = client.client_id();
        self.clients.insert(client_id, client);
        self.default_client_id = Some(client_id);
        log::debug!("Registered default client {client_id}");
    }

    /// Marks an already-registered client as the default for fallback routing.
    ///
    /// # Errors
    ///
    /// Returns an error if no client is registered with the given ID, or a different
    /// client is already the default.
    pub fn set_default_client(&mut self, client_id: ClientId) -> anyhow::Result<()> {
        if self.default_client_id.is_some_and(|id| id != client_id) {
            anyhow::bail!("default client already registered");
        }

        if !self.clients.contains_key(&client_id) {
            anyhow::bail!("No client registered with ID {client_id}");
        }

        self.default_client_id = Some(client_id);
        log::debug!("Set client {client_id} as default");
        Ok(())
    }

    /// Sets routing for a specific venue to a given client ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the client ID is not registered, or the venue is already routed to a
    /// different client.
    pub fn register_venue_routing(
        &mut self,
        client_id: ClientId,
        venue: Venue,
    ) -> anyhow::Result<()> {
        if !self.clients.contains_key(&client_id) {
            anyhow::bail!("No client registered with ID {client_id}");
        }

        if let Some(existing_client_id) = self.routing_map.get(&venue)
            && *existing_client_id != client_id
        {
            anyhow::bail!(
                "Venue {venue} already routed to {existing_client_id}, \
                 cannot re-route to {client_id}"
            );
        }

        self.routing_map.insert(venue, client_id);
        log::debug!("Set client {client_id} routing for {venue}");
        Ok(())
    }

    /// Returns connection status for each registered client.
    #[must_use]
    pub fn client_connection_status(&self) -> Vec<(ClientId, bool)> {
        self.get_clients()
            .into_iter()
            .map(|client| (client.client_id(), client.is_connected()))
            .collect()
    }

    /// Returns a list of all registered client IDs, including the default client if set.
    #[must_use]
    pub fn registered_clients(&self) -> Vec<ClientId> {
        self.get_clients()
            .into_iter()
            .map(|client| client.client_id())
            .collect()
    }

    pub(crate) fn collect_subscriptions<F, T>(&self, get_subs: F) -> Vec<T>
    where
        F: Fn(&DataClientAdapter) -> &AHashSet<T>,
        T: Clone,
    {
        self.get_clients()
            .into_iter()
            .flat_map(get_subs)
            .cloned()
            .collect()
    }

    #[must_use]
    pub fn get_clients(&self) -> Vec<&DataClientAdapter> {
        self.clients.values().collect()
    }

    #[must_use]
    pub fn get_clients_mut(&mut self) -> Vec<&mut DataClientAdapter> {
        self.clients.values_mut().collect()
    }

    pub fn get_client(
        &mut self,
        client_id: Option<&ClientId>,
        venue: Option<&Venue>,
    ) -> Option<&mut DataClientAdapter> {
        if let Some(client_id) = client_id {
            return self.clients.get_mut(client_id);
        }

        if let Some(v) = venue
            && let Some(client_id) = self.routing_map.get(v)
        {
            return self.clients.get_mut(client_id);
        }

        self.get_default_client()
    }

    /// Resolves the client for a subscribe/unsubscribe command.
    ///
    /// When `BACKTEST` is registered, all commands route through it regardless of
    /// the command's `client_id` or `venue`. Request paths skip this override.
    pub(super) fn get_command_client(
        &mut self,
        client_id: Option<&ClientId>,
        venue: Option<&Venue>,
    ) -> Option<&mut DataClientAdapter> {
        let backtest_id = ClientId::new("BACKTEST");
        if self.clients.contains_key(&backtest_id) {
            return self.clients.get_mut(&backtest_id);
        }

        self.get_client(client_id, venue)
    }

    fn get_default_client(&mut self) -> Option<&mut DataClientAdapter> {
        match self.default_client_id {
            Some(id) => self.clients.get_mut(&id),
            None => None,
        }
    }
}
