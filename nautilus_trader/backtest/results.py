from dataclasses import dataclass


@dataclass
class BacktestResult:
    """
    Represents the results of a single complete backtest run.
    """

    trader_id: str
    machine_id: str
    run_config_id: str | None
    instance_id: str
    run_id: str
    run_started: int | None
    run_finished: int | None
    backtest_start: int | None
    backtest_end: int | None
    elapsed_time: float
    iterations: int
    total_events: int
    total_orders: int
    total_positions: int
    stats_pnls: dict[str, dict[str, float]]
    stats_returns: dict[str, float]

    # account_balances: pd.DataFrame
    # fills_report: pd.DataFrame
    # positions: pd.DataFrame
    #
    # def final_balances(self):
    #     return self.account_balances.groupby(["venue", "currency"])["total"].last()
    #
    # def __repr__(self) -> str:
    #     def repr_balance():
    #         items = [
    #             (venue, currency, balance)
    #             for (venue, currency), balance in self.final_balances().items()
    #         ]
    #         return ",".join([f"{v.value}[{c}]={b}" for (v, c, b) in items])
    #
    #     return f"{self.__class__.__name__}({self.run_id}, {repr_balance()})"


def ensure_plotting(func):
    """
    Decorate a function that require a plotting library.

    Ensures library is installed and providers a better error about how to install if
    not found.

    """

    def inner(*args, **kwargs):
        try:
            import hvplot.pandas

            assert hvplot.pandas
        except ImportError:
            raise ImportError(
                "Failed to import plotting library - install in notebook via `%pip install hvplot`",
            )
        return func(*args, **kwargs)

    return inner


# @dataclass()
# class BacktestRunResults:
#     results: list[BacktestResult]
#
#     def final_balances(self):
#         return pd.concat(r.final_balances().to_frame().assign(id=r.id) for r in self.results)
#
#     @ensure_plotting
#     def plot_balances(self):
#         df = self.final_balances()
#         df = df.reset_index().set_index("id").astype({"venue": str, "total": float})
#         return df.hvplot.bar(y="total", rot=45, by=["venue", "currency"])
