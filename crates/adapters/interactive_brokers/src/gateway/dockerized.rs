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

//! Dockerized IB Gateway management.

#[cfg(feature = "gateway")]
use std::{collections::HashMap, fmt::Debug, time::Duration};

#[cfg(feature = "gateway")]
use anyhow::Context;
#[cfg(feature = "gateway")]
use bollard::Docker;
#[cfg(feature = "gateway")]
use bollard::container::{
    Config, CreateContainerOptions, LogOutput, LogsOptions, RemoveContainerOptions,
};
#[cfg(feature = "gateway")]
use bollard::models::{
    ContainerCreateResponse, HostConfig, PortBinding, RestartPolicy, RestartPolicyNameEnum,
};
#[cfg(feature = "gateway")]
use futures_util::StreamExt;
#[cfg(feature = "gateway")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "gateway")]
use crate::config::DockerizedIBGatewayConfig;
#[cfg(feature = "gateway")]

/// Container status enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum ContainerStatus {
    /// No container exists.
    NoContainer = 1,
    /// Container has been created but not started.
    ContainerCreated = 2,
    /// Container is starting.
    ContainerStarting = 3,
    /// Container has stopped.
    ContainerStopped = 4,
    /// Container is running but not logged in.
    NotLoggedIn = 5,
    /// Container is ready (running and logged in).
    Ready = 6,
    /// Unknown container status.
    Unknown = 7,
}

/// Dockerized IB Gateway manager.
///
/// This struct manages the lifecycle of Interactive Brokers Gateway Docker containers,
/// including creation, starting, stopping, and status checking.
#[derive(Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
#[cfg(feature = "gateway")]
pub struct DockerizedIBGateway {
    /// Configuration for the gateway.
    config: DockerizedIBGatewayConfig,
    /// Docker client.
    pub(crate) docker: Docker,
    /// Username for IB account.
    username: String,
    /// Password for IB account.
    password: String,
    /// Host address (always 127.0.0.1).
    host: String,
    /// Port for the gateway.
    port: u16,
    /// Container name.
    container_name: String,
}

#[cfg(feature = "gateway")]
impl Debug for DockerizedIBGateway {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(DockerizedIBGateway))
            .field("host", &self.host)
            .field("port", &self.port)
            .field("container_name", &self.container_name)
            .field("trading_mode", &self.config.trading_mode)
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "gateway")]
impl DockerizedIBGateway {
    /// Base container name.
    pub const CONTAINER_NAME: &'static str = "nautilus-ib-gateway";

    /// Host API ports by trading mode.
    pub const HOST_PORTS: &'static [(&'static str, u16)] = &[("Paper", 4002), ("Live", 4001)];

    /// Container API ports exposed by the IB Gateway image.
    pub const CONTAINER_PORTS: &'static [(&'static str, u16)] = &[("Paper", 4004), ("Live", 4003)];

    /// Internal VNC port.
    pub const VNC_PORT_INTERNAL: u16 = 5900;

    fn host_port_for_mode(trading_mode: crate::config::TradingMode) -> u16 {
        match trading_mode {
            crate::config::TradingMode::Paper => 4002,
            crate::config::TradingMode::Live => 4001,
        }
    }

    fn container_port_for_mode(trading_mode: crate::config::TradingMode) -> u16 {
        match trading_mode {
            crate::config::TradingMode::Paper => 4004,
            crate::config::TradingMode::Live => 4003,
        }
    }

    fn logs_indicate_ready(logs: &str) -> bool {
        logs.contains("Login has completed")
            || logs.contains("Configuration tasks completed")
            || logs.contains("Logged in to")
            || logs.contains("Login successful")
    }

    /// Create a new DockerizedIBGateway from configuration.
    ///
    /// # Arguments
    ///
    /// * `config` - Configuration for the gateway
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Username or password is not provided and not available in environment variables
    /// - Docker client creation fails
    pub fn new(config: DockerizedIBGatewayConfig) -> anyhow::Result<Self> {
        // Load username from config or environment (clone to avoid partial move)
        let username = config
            .username
            .clone()
            .or_else(|| std::env::var("TWS_USERNAME").ok())
            .ok_or_else(|| anyhow::anyhow!("username not set nor available in env TWS_USERNAME"))?;

        // Load password from config or environment (clone to avoid partial move)
        let password = config
            .password
            .clone()
            .or_else(|| std::env::var("TWS_PASSWORD").ok())
            .ok_or_else(|| anyhow::anyhow!("password not set nor available in env TWS_PASSWORD"))?;

        // Connect to Docker
        let docker = Docker::connect_with_local_defaults().context(
            "Failed to connect to the local Docker daemon. Ensure Docker is running and the local Docker socket is available",
        )?;

        // Determine port based on trading mode
        let mode_str = match config.trading_mode {
            crate::config::TradingMode::Paper => "Paper",
            crate::config::TradingMode::Live => "Live",
        };
        let port = Self::host_port_for_mode(config.trading_mode);

        // Generate container name
        let container_name = format!("{}-{}", Self::CONTAINER_NAME, mode_str).to_lowercase();

        Ok(Self {
            config,
            docker,
            username,
            password,
            host: "127.0.0.1".to_string(),
            port,
            container_name,
        })
    }

    /// Get the container name.
    pub fn container_name(&self) -> &str {
        &self.container_name
    }

    /// Get the host address.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Get the port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Check if the container is logged in by examining logs.
    ///
    /// # Arguments
    ///
    /// * `container_id` - The container ID to check
    ///
    /// # Errors
    ///
    /// Returns an error if log retrieval fails.
    pub async fn is_logged_in(&self, container_id: &str) -> anyhow::Result<bool> {
        let logs_options = LogsOptions::<String> {
            stdout: true,
            stderr: true,
            ..Default::default()
        };

        let mut logs_stream = self.docker.logs(container_id, Some(logs_options));

        let mut logged_in = false;

        while let Some(log_result) = logs_stream.next().await {
            let log_output = log_result.context("Failed to read log chunk")?;
            // Handle LogOutput enum variants
            let log_bytes = match log_output {
                LogOutput::StdOut { message } | LogOutput::StdErr { message } => message,
                LogOutput::StdIn { message } | LogOutput::Console { message } => message,
            };
            let log_string = String::from_utf8_lossy(&log_bytes);
            if Self::logs_indicate_ready(&log_string) {
                logged_in = true;
                break;
            }
        }

        Ok(logged_in)
    }

    /// Get the current container status.
    ///
    /// # Errors
    ///
    /// Returns an error if container inspection fails.
    pub async fn container_status(&self) -> anyhow::Result<ContainerStatus> {
        let list_options = bollard::container::ListContainersOptions::<String> {
            all: true,
            ..Default::default()
        };
        let containers = self
            .docker
            .list_containers(Some(list_options))
            .await
            .context("Failed to list containers")?;

        let container = containers.iter().find(|c| {
            c.names
                .as_ref()
                .and_then(|names| names.first())
                .map(|name| name.trim_start_matches('/') == self.container_name)
                .unwrap_or(false)
        });

        let Some(container) = container else {
            return Ok(ContainerStatus::NoContainer);
        };

        let state = container.state.as_deref().unwrap_or("unknown");

        match state {
            "running" => {
                let container_id = container
                    .id
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Container ID missing"))?;

                if self.is_logged_in(container_id).await.unwrap_or(false) {
                    Ok(ContainerStatus::Ready)
                } else {
                    Ok(ContainerStatus::ContainerStarting)
                }
            }
            "stopped" | "exited" => Ok(ContainerStatus::ContainerStopped),
            "created" => Ok(ContainerStatus::ContainerCreated),
            _ => Ok(ContainerStatus::Unknown),
        }
    }

    /// Start the gateway container.
    ///
    /// # Arguments
    ///
    /// * `wait` - Optional wait time in seconds (overrides config timeout)
    ///
    /// # Errors
    ///
    /// Returns an error if container creation or startup fails.
    pub async fn start(&mut self, wait: Option<u64>) -> anyhow::Result<()> {
        tracing::info!("Ensuring gateway is running");

        let status = self.container_status().await?;

        let broken_statuses = [
            ContainerStatus::NotLoggedIn,
            ContainerStatus::ContainerStopped,
            ContainerStatus::ContainerCreated,
            ContainerStatus::Unknown,
        ];

        match status {
            ContainerStatus::NoContainer => {
                tracing::debug!("No container, starting");
            }
            status if broken_statuses.contains(&status) => {
                tracing::debug!("Status {:?}, removing existing container", status);
                self.stop().await?;
            }
            ContainerStatus::Ready | ContainerStatus::ContainerStarting => {
                tracing::info!("Status {:?}, using existing container", status);
                return Ok(());
            }
            _ => {}
        }

        tracing::debug!("Starting new container");

        // Determine port mappings
        let host_port = Self::host_port_for_mode(self.config.trading_mode);
        let container_port = Self::container_port_for_mode(self.config.trading_mode);

        let mut port_bindings = HashMap::new();
        port_bindings.insert(
            format!("{}/tcp", container_port),
            Some(vec![PortBinding {
                host_ip: Some(self.host.clone()),
                host_port: Some(host_port.to_string()),
            }]),
        );

        if let Some(vnc_port) = self.config.vnc_port {
            port_bindings.insert(
                format!("{}/tcp", Self::VNC_PORT_INTERNAL),
                Some(vec![PortBinding {
                    host_ip: Some(self.host.clone()),
                    host_port: Some(vnc_port.to_string()),
                }]),
            );
        }

        // Prepare environment variables
        let mode_str = match self.config.trading_mode {
            crate::config::TradingMode::Paper => "paper",
            crate::config::TradingMode::Live => "live",
        };
        let env = vec![
            format!("TWS_USERID={}", self.username),
            format!("TWS_PASSWORD={}", self.password),
            format!("TRADING_MODE={}", mode_str),
            format!(
                "READ_ONLY_API={}",
                if self.config.read_only_api {
                    "yes"
                } else {
                    "no"
                }
            ),
            "EXISTING_SESSION_DETECTED_ACTION=primary".to_string(),
        ];

        // Create container configuration
        let container_config = Config {
            image: Some(self.config.container_image.clone()),
            hostname: Some(self.container_name.clone()),
            host_config: Some(HostConfig {
                port_bindings: Some(port_bindings),
                restart_policy: Some(RestartPolicy {
                    name: Some(RestartPolicyNameEnum::ALWAYS),
                    maximum_retry_count: None,
                }),
                ..Default::default()
            }),
            env: Some(env),
            ..Default::default()
        };

        // Create container
        let create_options = CreateContainerOptions {
            name: self.container_name.clone(),
            platform: None,
        };

        let create_response: ContainerCreateResponse = self
            .docker
            .create_container(Some(create_options), container_config)
            .await
            .context("Failed to create container")?;

        let container_id = create_response.id;

        // Start container
        self.docker
            .start_container(
                &container_id,
                None::<bollard::container::StartContainerOptions<String>>,
            )
            .await
            .context("Failed to start container")?;

        tracing::info!(
            "Container `{}` starting, waiting for ready",
            self.container_name
        );

        // Wait for container to be ready
        let wait_time = wait.unwrap_or(self.config.timeout);
        let mut waited = 0u64;

        while waited < wait_time {
            if self.is_logged_in(&container_id).await.unwrap_or(false) {
                tracing::info!(
                    "Gateway `{}` ready. VNC port is {:?}",
                    self.container_name,
                    self.config.vnc_port
                );
                return Ok(());
            }

            tracing::debug!("Waiting for IB Gateway to start");
            tokio::time::sleep(Duration::from_secs(1)).await;
            waited += 1;
        }

        anyhow::bail!(
            "Gateway `{}` not ready after {} seconds",
            self.container_name,
            wait_time
        )
    }

    /// Safely start the gateway, handling container already exists errors.
    ///
    /// # Arguments
    ///
    /// * `wait` - Optional wait time in seconds
    ///
    /// # Errors
    ///
    /// Returns an error if startup fails (other than container exists).
    pub async fn safe_start(&mut self, wait: Option<u64>) -> anyhow::Result<()> {
        match self.start(wait).await {
            Ok(()) => Ok(()),
            Err(e) if e.to_string().contains("already exists") => {
                tracing::warn!("Container already exists, continuing");
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// Stop and remove the gateway container.
    ///
    /// # Errors
    ///
    /// Returns an error if container stop or removal fails.
    pub async fn stop(&self) -> anyhow::Result<()> {
        let list_options = bollard::container::ListContainersOptions::<String> {
            all: true,
            ..Default::default()
        };
        let containers = self
            .docker
            .list_containers(Some(list_options))
            .await
            .context("Failed to list containers")?;

        let container = containers.iter().find(|c| {
            c.names
                .as_ref()
                .and_then(|names| names.first())
                .map(|name| name.trim_start_matches('/') == self.container_name)
                .unwrap_or(false)
        });

        if let Some(container) = container {
            if let Some(container_id) = &container.id {
                // Stop container if running
                if container.state.as_deref() == Some("running") {
                    self.docker
                        .stop_container(
                            container_id,
                            None::<bollard::container::StopContainerOptions>,
                        )
                        .await
                        .context("Failed to stop container")?;
                }

                // Remove container
                let remove_options = RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                };

                self.docker
                    .remove_container(container_id, Some(remove_options))
                    .await
                    .context("Failed to remove container")?;

                tracing::info!("Stopped and removed container `{}`", self.container_name);
            }
        }

        Ok(())
    }
}

/// Stub implementation when gateway feature is disabled.
#[cfg(not(feature = "gateway"))]
#[derive(Debug)]
pub struct DockerizedIBGateway;

#[cfg(not(feature = "gateway"))]
impl DockerizedIBGateway {
    /// # Errors
    ///
    /// Returns an error if the Dockerized IB Gateway cannot be created or started.
    pub fn new(_config: crate::config::DockerizedIBGatewayConfig) -> anyhow::Result<Self> {
        anyhow::bail!("Gateway feature is not enabled. Build with --features gateway")
    }
}

#[cfg(all(test, feature = "gateway"))]
mod tests {
    use rstest::rstest;

    use super::DockerizedIBGateway;
    use crate::config::TradingMode;

    #[rstest]
    #[case(TradingMode::Paper, 4002)]
    #[case(TradingMode::Live, 4001)]
    fn host_port_matches_trading_mode(#[case] trading_mode: TradingMode, #[case] expected: u16) {
        assert_eq!(
            DockerizedIBGateway::host_port_for_mode(trading_mode),
            expected
        );
    }

    #[rstest]
    #[case(TradingMode::Paper, 4004)]
    #[case(TradingMode::Live, 4003)]
    fn container_port_matches_trading_mode(
        #[case] trading_mode: TradingMode,
        #[case] expected: u16,
    ) {
        assert_eq!(
            DockerizedIBGateway::container_port_for_mode(trading_mode),
            expected
        );
    }

    #[rstest]
    #[case(TradingMode::Paper, 4002)]
    #[case(TradingMode::Live, 4001)]
    fn new_reports_the_host_api_port(#[case] trading_mode: TradingMode, #[case] expected: u16) {
        let gateway = DockerizedIBGateway::new(
            crate::config::DockerizedIBGatewayConfig::builder()
                .username("test-user".to_string())
                .password("test-password".to_string())
                .trading_mode(trading_mode)
                .build(),
        )
        .unwrap();

        assert_eq!(gateway.port(), expected);
    }

    #[rstest]
    #[case("Forking ::: Starting IBC Gateway", false)]
    #[case("Started IB Gateway", false)]
    #[case("Login has completed", true)]
    #[case("Configuration tasks completed", true)]
    #[case("Logged in to backend", true)]
    #[case("Login successful", true)]
    fn ready_log_markers_are_strict(#[case] logs: &str, #[case] expected: bool) {
        assert_eq!(DockerizedIBGateway::logs_indicate_ready(logs), expected);
    }
}
