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
"""
Configuration for margin calculation models.
"""

from nautilus_trader.common.config import NautilusConfig


class MarginModelConfig(NautilusConfig, frozen=True):
    """
    Configuration for margin calculation models.

    Parameters
    ----------
    model_type : str
        The type of margin model to use. Options:
        - "standard": Fixed percentages without leverage division (traditional brokers)
        - "leveraged": Margin requirements reduced by leverage (current Nautilus behavior)
        - Custom class path for custom models
    config : dict, optional
        Additional configuration parameters for custom models.

    """

    model_type: str = "leveraged"  # Default to current behavior for backward compatibility
    config: dict = {}


class MarginModelFactory:
    """
    Provides margin model creation from configurations.
    """

    @staticmethod
    def create(config: MarginModelConfig):
        """
        Create a margin model from the given configuration.

        Parameters
        ----------
        config : MarginModelConfig
            The configuration for the margin model.

        Returns
        -------
        MarginModel
            The created margin model instance.

        Raises
        ------
        ValueError
            If the model type is unknown or invalid.

        """
        from nautilus_trader.accounting.margin_models import LeveragedMarginModel
        from nautilus_trader.accounting.margin_models import StandardMarginModel

        model_type = config.model_type.lower()

        if model_type == "standard":
            return StandardMarginModel()
        elif model_type == "leveraged":
            return LeveragedMarginModel()
        else:
            # Try to import custom model
            try:
                from nautilus_trader.common.config import resolve_path

                model_cls = resolve_path(config.model_type)
                return model_cls(config)
            except Exception as e:
                raise ValueError(
                    f"Unknown margin model type '{config.model_type}'. "
                    f"Supported types: 'standard', 'leveraged', "
                    f"or a fully qualified class path. Error: {e}",
                ) from e
