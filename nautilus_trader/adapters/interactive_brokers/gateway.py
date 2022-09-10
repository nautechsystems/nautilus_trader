# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
import warnings
from enum import IntEnum
from time import sleep
from typing import Optional


try:
    import docker
except ImportError as e:
    warnings.warn(
        f"Docker required for Gateway, please install manually via `pip install docker` ({e})"
    )
    docker = None
from ib_insync import IB


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
    A class to manage starting an Interactive Brokers Gateway docker container
    """

    IMAGE = "ghcr.io/unusualalpha/ib-gateway"
    CONTAINER_NAME = "nautilus-ib-gateway"
    PORTS = {"paper": 4002, "live": 4001}

    def __init__(
        self,
        username: str,
        password: str,
        host: Optional[str] = "localhost",
        port: Optional[int] = None,
        trading_mode: Optional[str] = "paper",
        start: bool = False,
        logger: Optional[logging.Logger] = None,
    ):
        self.username = username
        self.password = password
        self.trading_mode = trading_mode
        self.host = host
        self.port = port or self.PORTS[trading_mode]
        if docker is None:
            raise RuntimeError("Docker not installed")
        self._docker = docker.from_env()
        self._client: Optional[IB] = None
        self._container = None
        self.log = logger or logging.getLogger("nautilus_trader")
        if start:
            self.start()

    @classmethod
    def from_container(cls, **kwargs):
        """Connect to an already running container - don't stop/start"""
        self = cls(username="", password="", **kwargs)  # noqa: S106
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
            self._container = all_containers.get(self.CONTAINER_NAME)
        return self._container

    @property
    def client(self) -> IB:
        if self._client is None:
            self._client = IB()
            self._client.connect(host=self.host, port=self.port)
        return self._client

    @staticmethod
    def is_logged_in(container) -> bool:
        try:
            logs = container.logs()
        except NoContainer:
            return False
        return any([b"Forking :::" in line for line in logs.split(b"\n")])

    def start(self, wait: Optional[int] = 90):
        """
        :param wait: Seconds to wait until container is ready
        :return:
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
            raise ContainerExists

        self.log.debug("Starting new container")
        self._container = self._docker.containers.run(
            image=self.IMAGE,
            name=self.CONTAINER_NAME,
            detach=True,
            ports={"4001": "4001", "4002": "4002", "5900": "5900"},
            platform="amd64",
            environment={
                "TWSUSERID": self.username,
                "TWSPASSWORD": self.password,
                "TRADING_MODE": self.trading_mode,
            },
        )
        self.log.info("Container starting, waiting for ready")

        if wait is not None:
            for _ in range(wait):
                if self.is_logged_in(container=self._container):
                    break
                else:
                    self.log.debug("Waiting for IB Gateway to start ..")
                    sleep(1)
            else:
                raise GatewayLoginFailure

        self.log.info("Gateway ready")

    def safe_start(self, wait: int = 90):
        try:
            self.start(wait=wait)
        except ContainerExists:
            return

    def stop(self):
        if self.container:
            self.container.stop()
            self.container.remove()

    def __enter__(self):
        self.start()

    def __exit__(self, type, value, traceback):
        self.stop()


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
