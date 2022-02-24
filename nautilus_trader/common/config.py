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

import importlib
import importlib.util
from typing import Any, Dict, FrozenSet, Optional, Union

import pydantic
from frozendict import frozendict
from pydantic import validator

from nautilus_trader.core.correctness import PyCondition


class ActorConfig(pydantic.BaseModel):
    """
    The base model for all actor configurations.

    Parameters
    ----------
    component_id : str, optional
        The component ID. If ``None`` then the identifier will be taken from
        `type(self).__name__`.

    """

    component_id: Optional[str] = None


class ImportableActorConfig(pydantic.BaseModel):
    """
    Represents an actor configuration for one specific backtest run.

    Parameters
    ----------
    path : str, optional
        The fully qualified name of the module.
    config : Union[ActorConfig, str]

    """

    path: Optional[str]
    config: Union[ActorConfig, str]

    def _check_path(self):
        assert self.path, "`path` not set, can't parse module"
        assert ":" in self.path, "Path variable should be of the form: path.to.module:class"

    @property
    def module(self):
        self._check_path()
        return self.path.rsplit(":")[0]

    @property
    def cls(self):
        self._check_path()
        return self.path.rsplit(":")[1]


class ActorFactory:
    """
    Provides actor creation from importable configurations.
    """

    @staticmethod
    def create(config: ImportableActorConfig):
        """
        Create an actor from the given configuration.

        Parameters
        ----------
        config : ImportableActorConfig
            The configuration for the building step.

        Returns
        -------
        Actor

        Raises
        ------
        TypeError
            If `config` is not of type `ImportableActorConfig`.

        """
        PyCondition.type(config, ImportableActorConfig, "config")
        mod = importlib.import_module(config.module)
        cls = getattr(mod, config.cls)
        assert isinstance(config.config, ActorConfig)
        return cls(config=config.config)


class InstrumentProviderConfig(pydantic.BaseModel):
    """
    Configuration for ``InstrumentProvider`` instances.

    Parameters
    ----------
    load_all : bool, default False
        If all venue instruments should be loaded on start.
    load_ids : FrozenSet[str], optional
        The list of instrument IDs to be loaded on start (if `load_all_instruments` is False).
    filters : frozendict, optional
        The venue specific instrument loading filters to apply.
    """

    class Config:
        """The base model config"""

        arbitrary_types_allowed = True

    @validator("filters")
    def validate_filters(cls, value):
        return frozendict(value)

    def __eq__(self, other):
        return (
            self.load_all == other.load_all
            and self.load_ids == other.load_ids
            and self.filters == other.filters
        )

    def __hash__(self):
        return hash((self.load_all, self.load_ids, self.filters))

    load_all: bool = False
    load_ids: Optional[FrozenSet[str]] = None
    filters: Optional[Dict[str, Any]] = None
