# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import logging
import os
from enum import IntEnum
from time import sleep
from typing import ClassVar

from nautilus_trader.adapters.interactive_brokers.config import DockerizedIBGatewayConfig
from nautilus_trader.common.component import Logger as NautilusLogger
from nautilus_trader.common.secure import SecureString


class ContainerStatus(IntEnum):
    NO_CONTAINER = 1
    CONTAINER_CREATED = 2
    CONTAINER_STARTING = 3
    CONTAINER_STOPPED = 4
    NOT_LOGGED_IN = 5
    READY = 6
    UNKNOWN = 7


class DockerizedIBGateway:
    """
    A class to manage starting an Interactive Brokers Gateway docker container.
    """

    CONTAINER_NAME: ClassVar[str] = "nautilus-ib-gateway"
    PORTS_INTERNAL: ClassVar[dict[str, int]] = {"paper": 4002, "live": 4001}
    PORTS_EXTERNAL: ClassVar[dict[str, int]] = {"paper": 4004, "live": 4003}
    VNC_PORT_INTERNAL: ClassVar[int] = 5900

    def __init__(self, config: DockerizedIBGatewayConfig):
        self.log = NautilusLogger(repr(self))
        self.username = config.username or os.getenv("TWS_USERNAME")

        password = config.password or os.getenv("TWS_PASSWORD")
        if self.username is None:
            self.log.error("`username` not set nor available in env `TWS_USERNAME`")
            raise ValueError("`username` not set nor available in env `TWS_USERNAME`")

        if password is None:
            self.log.error("`password` not set nor available in env `TWS_PASSWORD`")
            raise ValueError("`password` not set nor available in env `TWS_PASSWORD`")

        self.password = SecureString(password, name="tws_password")

        self.trading_mode = config.trading_mode
        self.read_only_api = config.read_only_api
        self.host = "127.0.0.1"
        self.port = self.PORTS_INTERNAL[config.trading_mode]
        self.timeout = config.timeout
        self.container_image = config.container_image
        self.vnc_port = config.vnc_port

        try:
            import docker

            self._docker_module = docker
        except ImportError as e:
            raise RuntimeError(
                "Docker required for Gateway, install via `pip install docker`",
            ) from e

        self._docker = docker.from_env()
        self._container = None

    def __repr__(self):
        return f"{type(self).__name__}"

    @property
    def container_name(self) -> str:
        """
        Return the name of the Docker container for the IB Gateway instance.

        Returns
        -------
        str
            The container name, which is constructed using the base container name and the trading mode
            corresponding to the current trading mode.

            e.g. "nautilus-ib-gateway-paper" or "nautilus-ib-gateway-live"

        """
        return f"{self.CONTAINER_NAME}-{self.trading_mode}"

    @property
    def container_status(self) -> ContainerStatus:
        container = self.container

        if container is None:
            return ContainerStatus.NO_CONTAINER
        elif container.status == "running":
            if self.is_logged_in(container=container):
                return ContainerStatus.READY
            else:
                return ContainerStatus.CONTAINER_STARTING
        elif container.status in ("stopped", "exited"):
            return ContainerStatus.CONTAINER_STOPPED
        else:
            return ContainerStatus.UNKNOWN

    @property
    def container(self):
        if self._container is None:
            all_containers = {c.name: c for c in self._docker.containers.list(all=True)}
            self._container = all_containers.get(self.container_name)

        return self._container

    @staticmethod
    def is_logged_in(container) -> bool:
        try:
            logs = container.logs()
        except NoContainer:
            return False

        return any(b"Forking :::" in line for line in logs.split(b"\n"))

    def start(self, wait: int | None = None) -> None:
        """
        Start the gateway.

        Parameters
        ----------
        wait : int, optional
            The seconds to wait until container is ready.

        """
        broken_statuses = (
            ContainerStatus.NOT_LOGGED_IN,
            ContainerStatus.CONTAINER_STOPPED,
            ContainerStatus.CONTAINER_CREATED,
            ContainerStatus.UNKNOWN,
        )
        self.log.info("Ensuring gateway is running")
        status = self.container_status

        if status == ContainerStatus.NO_CONTAINER:
            self.log.debug("No container, starting")
        elif status in broken_statuses:
            self.log.debug(f"{status=}, removing existing container")
            self.stop()
        elif status in (ContainerStatus.READY, ContainerStatus.CONTAINER_STARTING):
            self.log.info(f"{status=}, using existing container")
            return

        self.log.debug("Starting new container")

        ports = {
            str(self.PORTS_EXTERNAL[self.trading_mode]): (self.host, self.port),
        }

        if self.vnc_port is not None:
            ports[str(self.VNC_PORT_INTERNAL)] = (self.host, self.vnc_port)

        self._container = self._docker.containers.run(
            image=self.container_image,
            name=self.container_name,
            restart_policy={"Name": "always"},
            detach=True,
            ports=ports,
            platform="amd64",
            environment={
                "TWS_USERID": self.username,
                "TWS_PASSWORD": self.password.get_value(),
                "TRADING_MODE": self.trading_mode,
                "READ_ONLY_API": {True: "yes", False: "no"}[self.read_only_api],
            },
        )
        self.log.info(f"Container `{self.container_name}` starting, waiting for ready")

        for _ in range(wait or self.timeout):
            if self.is_logged_in(container=self._container):
                break

            self.log.debug("Waiting for IB Gateway to start")
            sleep(1)
        else:
            raise RuntimeError(f"Gateway `{self.container_name}` not ready")

        self.log.info(
            f"Gateway `{self.container_name}` ready. VNC port is {self.vnc_port}",
        )

    def safe_start(self, wait: int | None = None) -> None:
        try:
            self.start(wait=wait)
        except self._docker_module.errors.APIError as e:
            raise RuntimeError("Container already exists") from e

    def stop(self) -> None:
        if self.container:
            self.container.stop()
            self.container.remove()

    def __enter__(self):
        self.start()

    def __exit__(self, exc_type, exc_val, exc_tb):
        try:
            self.stop()
        except Exception:
            logging.exception("Error stopping container")


# -- Exceptions -----------------------------------------------------------------------------------


class ContainerExists(Exception):
    pass


class NoContainer(Exception):
    pass


class UnknownContainerStatus(Exception):
    pass


class GatewayLoginFailure(Exception):
    pass


__all__ = ["DockerizedIBGateway"]
