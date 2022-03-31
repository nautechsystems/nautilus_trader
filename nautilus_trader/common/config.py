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
from typing import Any, Dict, FrozenSet, Optional

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


def resolve_path(path: str):
    module, cls = path.rsplit(":", maxsplit=1)
    mod = importlib.import_module(module)
    cls = getattr(mod, cls)
    return cls


class ImportableConfig(pydantic.BaseModel):
    """
    Base class for ImportableConfig.
    """

    @staticmethod
    def is_importable(data: Dict):
        return set(data) == {"factory_path", "config_path", "config"}

    @staticmethod
    def create(data: Dict, config_type: type):
        assert (
            ":" in data["factory_path"]
        ), "`class_path` variable should be of the form `path.to.module:class`"
        assert (
            ":" in data["config_path"]
        ), "`config_path` variable should be of the form `path.to.module:class`"
        cls = resolve_path(data["config_path"])
        config = cls(**data["config"])
        assert isinstance(config, config_type)
        return config


class ImportableActorConfig(pydantic.BaseModel):
    """
    Represents an actor configuration for one specific backtest run.

    Parameters
    ----------
    actor_path : str
        The fully qualified name of the Actor class.
    config_path : str
        The fully qualified name of the Actor Config class.
    config : Dict
        The actor configuration
    """

    actor_path: str
    config_path: str
    config: dict


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
        strategy_cls = resolve_path(config.actor_path)
        config_cls = resolve_path(config.config_path)
        return strategy_cls(config=config_cls(**config.config))


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
        return frozendict(value) if value is not None else None

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
