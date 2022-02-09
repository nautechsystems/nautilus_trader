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


import os
from enum import IntEnum
from time import sleep
from typing import Optional

from docker import DockerClient
from docker.models.containers import Container
from ib_insync import IB


class ContainerStatus(IntEnum):
    NO_CONTAINER = 1
    CONTAINER_STOPPED = 2
    NOT_LOGGED_IN = 3
    RUNNING = 4


class InteractiveBrokersGateway:
    """
    A context manager for starting an IB Gateway docker container
    """

    IMAGE = "mgvazquez/ibgateway"
    CONTAINER_NAME = "nautilus-ib-gateway"

    def __init__(
        self,
        username: str,
        password: str,
        host="localhost",
        port=4001,
        trading_mode="paper",
        start=False,
    ):
        self.username = username
        self.password = password
        self.trading_mode = trading_mode
        self.host = host
        self.port = port
        self._docker: DockerClient = DockerClient.from_env()
        self._client: Optional[IB] = None
        self._container = None
        if start:
            self.start()

    @classmethod
    def from_env(cls, **kwargs):
        return cls(
            username=os.environ["TWS_USERNAME"], password=os.environ["TWS_PASSWORD"], **kwargs
        )

    @classmethod
    def from_container(cls, **kwargs):
        """Connect to an already running container - don't stop/start"""
        self = cls(username="", password="", **kwargs)  # noqa: S106
        assert self.container, "Container does not exist"
        return self

    @property
    def container(self) -> Container:
        if self._container is None:
            all_containers = {c.name: c for c in self._docker.containers.list(all=True)}
            container = all_containers.get(self.CONTAINER_NAME)
            if container is None:
                raise NoContainer
            elif container.status == "running":
                self._container = container
            elif container.status in ("created", "stopped", "exited"):
                container.remove(force=True)
            else:
                raise UnknownContainerStatus
        return self._container

    @property
    def client(self) -> IB:
        if self._client is None:
            self._client = IB()
            self._client.connect(host=self.host, port=self.port)
        return self._client

    @property
    def is_logged_in(self) -> bool:
        try:
            logs = self.container.logs()
        except NoContainer:
            return False
        return any([b"Login has completed" in line for line in logs.split(b"\n")])

    def start(self, reset=False, wait: Optional[int] = 30):
        """
        :param reset: Stop and start the container
        :param wait: Seconds to wait until container is ready
        :return:
        """
        print("Starting gateway container")
        if self.container:
            if not reset:
                if not self.is_logged_in:
                    raise GatewayLoginFailure
                raise ContainerExists(
                    "Container already exists, skipping start. Use reset=True to force restart"
                )
            else:
                self.stop()
        self._container = self._docker.containers.run(
            image=self.IMAGE,
            name=self.CONTAINER_NAME,
            detach=True,
            ports={"4001": "4001"},
            environment={
                "TWSUSERID": self.username,
                "TWSPASSWORD": self.password,
                "TRADING_MODE": self.trading_mode,
            },
        )
        print("Container starting, waiting for ready")

        if wait is not None:
            for _ in range(wait):
                if self.is_logged_in:
                    break
                else:
                    sleep(1)
            else:
                raise GatewayLoginFailure
        print("Gateway ready")

    def safe_start(self):
        try:
            self.start()
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


# -------- Exceptions ---------------------------------------------------------------------------------------- #


class ContainerExists(Exception):
    pass


class NoContainer(Exception):
    pass


class UnknownContainerStatus(Exception):
    pass


class GatewayLoginFailure(Exception):
    pass


__all__ = ["InteractiveBrokersGateway"]
