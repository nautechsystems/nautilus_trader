# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.config import ActorFactory
from nautilus_trader.config import ImportableActorConfig
from nautilus_trader.test_kit.mocks.actors import MockActor


class TestActorFactory:
    def test_create_from_path(self):
        # Arrange
        config = {
            "component_id": "MyActor",
        }
        importable = ImportableActorConfig(
            actor_path="nautilus_trader.test_kit.mocks.actors:MockActor",
            config_path="nautilus_trader.test_kit.mocks.actors:MockActorConfig",
            config=config,
        )

        # Act
        actor = ActorFactory.create(importable)

        # Assert
        assert isinstance(actor, MockActor)
        assert repr(actor.config) == "MockActorConfig(component_id='MyActor')"
