from nautilus_trader.common.component import LogColor
from nautilus_trader.config import ExecAlgorithmConfig
from nautilus_trader.execution.algorithm import ExecAlgorithm
from nautilus_trader.model.identifiers import ExecAlgorithmId
from nautilus_trader.model.orders import Order
from nautilus_trader.model.orders import OrderList


class MyExecAlgorithmConfig(ExecAlgorithmConfig, frozen=True):
    """
    Configuration for ``MyExecAlgorithm`` instances.

    Parameters
    ----------
    exec_algorithm_id : str | ExecAlgorithmId, optional
        The execution algorithm ID (will override default which is the class name).

    """

    exec_algorithm_id: ExecAlgorithmId | None = None


class MyExecAlgorithm(ExecAlgorithm):
    """
    A blank template execution algorithm.

    Parameters
    ----------
    config : MyExecAlgorithmConfig
        The configuration for the instance.

    """

    def __init__(self, config: MyExecAlgorithmConfig) -> None:
        super().__init__(config)
        # Optionally implement further initialization

    def on_start(self) -> None:
        """
        Actions to be performed when the algorithm component is started.
        """
        # Optionally implement

    def on_stop(self) -> None:
        """
        Actions to be performed when the algorithm component is stopped.
        """
        # Optionally implement

    def on_reset(self) -> None:
        """
        Actions to be performed when the algorithm component is reset.
        """
        # Optionally implement

    def on_dispose(self) -> None:
        """
        Actions to be performed when the algorithm component is disposed.

        Cleanup any resources used by the strategy here.

        """
        # Optionally implement

    def on_save(self) -> dict[str, bytes]:
        """
        Actions to be performed when the algorithm component is saved.

        Create and return a state dictionary of values to be saved.

        Returns
        -------
        dict[str, bytes]
            The strategy state dictionary.

        """
        return {}  # Optionally implement

    def on_load(self, state: dict[str, bytes]) -> None:
        """
        Actions to be performed when the algorithm component is loaded.

        Saved state values will be contained in the give state dictionary.

        Parameters
        ----------
        state : dict[str, bytes]
            The strategy state dictionary.

        """
        # Optionally implement

    def on_order(self, order: Order) -> None:
        """
        Actions to be performed when running and receives an order.

        Parameters
        ----------
        order : Order
            The order to be handled.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        self.log.info(repr(order), LogColor.CYAN)
        # Optionally implement

    def on_order_list(self, order_list: OrderList) -> None:
        """
        Actions to be performed when running and receives an order list.

        Parameters
        ----------
        order_list : OrderList
            The order list to be handled.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        self.log.info(repr(order_list), LogColor.CYAN)
        # Optionally implement
