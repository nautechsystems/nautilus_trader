# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
from typing import ClassVar, Literal

from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersGatewayConfig


class ContainerStatus(IntEnum):
    NO_CONTAINER = 1
    CONTAINER_CREATED = 2
    CONTAINER_STARTING = 3
    CONTAINER_STOPPED = 4
    NOT_LOGGED_IN = 5
    READY = 6
    UNKNOWN = 7


class InteractiveBrokersGateway:
    """
    A class to manage starting an Interactive Brokers Gateway docker container.
    """

    IMAGE: ClassVar[str] = "ghcr.io/gnzsnz/ib-gateway:stable"
    CONTAINER_NAME: ClassVar[str] = "nautilus-ib-gateway"
    PORTS: ClassVar[dict[str, int]] = {"paper": 4002, "live": 4001}

    def __init__(
        self,
        username: str | None = None,
        password: str | None = None,
        host: str | None = "127.0.0.1",
        port: int | None = None,
        trading_mode: Literal["paper", "live"] | None = "paper",
        start: bool = False,
        read_only_api: bool = True,
        timeout: int = 90,
        logger: logging.Logger | None = None,
        config: InteractiveBrokersGatewayConfig | None = None,
    ):
        if config:
            username = config.username
            password = config.password
            host = config.host
            port = config.port
            trading_mode = config.trading_mode
            start = config.start
            read_only_api = config.read_only_api
            timeout = config.timeout

        self.username = username or os.getenv("TWS_USERNAME")
        self.password = password or os.getenv("TWS_PASSWORD")
        if self.username is None:
            raise ValueError("`username` not set nor available in env `TWS_USERNAME`")
        if self.password is None:
            raise ValueError("`password` not set nor available in env `TWS_PASSWORD`")

        self.trading_mode = trading_mode
        self.read_only_api = read_only_api
        self.host = host
        self.port = port or self.PORTS[trading_mode]
        self.log = logger or logging.getLogger("nautilus_trader")

        try:
            import docker

            self._docker_module = docker
        except ImportError as e:
            raise RuntimeError(
                "Docker required for Gateway, install via `pip install docker`",
            ) from e

        self._docker = docker.from_env()
        self._container = None
        if start:
            self.start(timeout)

    @classmethod
    def from_container(cls, **kwargs) -> "InteractiveBrokersGateway":
        """Connect to an already running container - don't stop/start"""
        self = cls(username="", password="", **kwargs)
        assert self.container, "Container does not exist"
        return self

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
            self._container = all_containers.get(f"{self.CONTAINER_NAME}-{self.port}")
        return self._container

    @staticmethod
    def is_logged_in(container) -> bool:
        try:
            logs = container.logs()
        except NoContainer:
            return False
        return any(b"Forking :::" in line for line in logs.split(b"\n"))

    def start(self, wait: int | None = 90) -> None:
        """
        Start the gateway.

        Parameters
        ----------
        wait : int, default 90
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
        self._container = self._docker.containers.run(
            image=self.IMAGE,
            name=f"{self.CONTAINER_NAME}-{self.port}",
            restart_policy={"Name": "always"},
            detach=True,
            ports={
                "4003": (self.host, 4001),
                "4004": (self.host, 4002),
                "5900": (self.host, 5900),
            },
            platform="amd64",
            environment={
                "TWS_USERID": self.username,
                "TWS_PASSWORD": self.password,
                "TRADING_MODE": self.trading_mode,
                "READ_ONLY_API": {True: "yes", False: "no"}[self.read_only_api],
            },
        )
        self.log.info(f"Container `{self.CONTAINER_NAME}-{self.port}` starting, waiting for ready")

        if wait is not None:
            for _ in range(wait):
                if self.is_logged_in(container=self._container):
                    break
                self.log.debug("Waiting for IB Gateway to start ..")
                sleep(1)
            else:
                raise RuntimeError(f"Gateway `{self.CONTAINER_NAME}-{self.port}` not ready")

        self.log.info(
            f"Gateway `{self.CONTAINER_NAME}-{self.port}` ready. VNC port is {self.port + 100}",
        )

    def safe_start(self, wait: int = 90) -> None:
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
        except Exception as e:
            logging.error("Error stopping container: %s", e)


# -- Exceptions -----------------------------------------------------------------------------------


class ContainerExists(Exception):
    pass


class NoContainer(Exception):
    pass


class UnknownContainerStatus(Exception):
    pass


class GatewayLoginFailure(Exception):
    pass


__all__ = ["InteractiveBrokersGateway"]
