# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
Check generated adapter types against their runtime Python interface.
"""

import ast
import inspect
from pathlib import Path

import pytest

import nautilus_trader.live as live  # noqa: PLR0402 - Load the Python package; the root attribute can refer to the native module.


def stub_classes() -> dict[str, ast.ClassDef]:
    """
    Read generated class definitions from the installed public module.
    """
    tree = ast.parse(Path(live.__file__).with_suffix(".pyi").read_text(encoding="utf-8"))
    return {node.name: node for node in tree.body if isinstance(node, ast.ClassDef)}


@pytest.mark.parametrize(
    "name",
    [
        "BarsResponse",
        "BatchCancelOrders",
        "BatchModifyOrders",
        "BookDeltasResponse",
        "BookDepthResponse",
        "BookResponse",
        "CancelAllOrders",
        "CancelOrder",
        "CustomDataResponse",
        "FundingRatesResponse",
        "GenerateFillReports",
        "GenerateOrderStatusReport",
        "GenerateOrderStatusReports",
        "GeneratePositionStatusReports",
        "InstrumentResponse",
        "InstrumentsResponse",
        "ModifyOrder",
        "OptionChainReferencePriceResponse",
        "QueryAccount",
        "QueryOrder",
        "QuotesResponse",
        "RequestBars",
        "RequestBookDeltas",
        "RequestBookDepth",
        "RequestBookSnapshot",
        "RequestCustomData",
        "RequestFundingRates",
        "RequestInstrument",
        "RequestInstruments",
        "RequestOptionChainReferencePrice",
        "RequestQuotes",
        "RequestTrades",
        "SubmitOrder",
        "SubmitOrderList",
        "SubscribeBars",
        "SubscribeBookDeltas",
        "SubscribeBookDepth10",
        "SubscribeCustomData",
        "SubscribeFundingRates",
        "SubscribeIndexPrices",
        "SubscribeInstrument",
        "SubscribeInstrumentClose",
        "SubscribeInstrumentStatus",
        "SubscribeInstruments",
        "SubscribeMarkPrices",
        "SubscribeOptionGreeks",
        "SubscribeQuotes",
        "SubscribeTrades",
        "TradesResponse",
        "UnsubscribeBars",
        "UnsubscribeBookDeltas",
        "UnsubscribeBookDepth10",
        "UnsubscribeCustomData",
        "UnsubscribeFundingRates",
        "UnsubscribeIndexPrices",
        "UnsubscribeInstrument",
        "UnsubscribeInstrumentClose",
        "UnsubscribeInstrumentStatus",
        "UnsubscribeInstruments",
        "UnsubscribeMarkPrices",
        "UnsubscribeOptionGreeks",
        "UnsubscribeQuotes",
        "UnsubscribeTrades",
    ],
)
def test_adapter_stub_names_and_properties_match_runtime(name: str) -> None:
    """
    Expose the runtime name and every native getter as a stub property.
    """
    runtime = getattr(live, name)
    classes = stub_classes()
    assert name in classes
    assert "Py" + name not in classes
    stub = classes[name]
    actual = {
        node.name
        for node in stub.body
        if isinstance(node, ast.FunctionDef)
        and any(
            isinstance(item, ast.Name) and item.id == "property" for item in node.decorator_list
        )
    }
    expected = {key for key, value in vars(runtime).items() if inspect.isgetsetdescriptor(value)}

    assert actual == expected
    assert runtime.__name__ == name
    assert runtime.__module__ == "nautilus_trader.live"


@pytest.mark.parametrize("name", ["DataClientConfig", "ExecutionClientConfig"])
def test_client_config_constructor_stub_preserves_subclass(name: str) -> None:
    """
    Describe the subclass returned by the runtime constructor.
    """
    runtime = getattr(live, name)

    class Derived(runtime):
        pass

    constructor = next(
        node
        for node in stub_classes()[name].body
        if isinstance(node, ast.FunctionDef) and node.name == "__new__"
    )

    assert type(Derived.__new__(Derived)) is Derived
    assert ast.unparse(constructor.returns) == "typing.Self"


def test_live_node_run_stub_matches_runtime_arguments() -> None:
    """
    Keep the existing no-argument run method consistent for static callers.
    """
    method = next(
        node
        for node in stub_classes()["LiveNode"].body
        if isinstance(node, ast.FunctionDef) and node.name == "run"
    )
    arguments = method.args.posonlyargs + method.args.args + method.args.kwonlyargs

    assert [argument.arg for argument in arguments] == ["self"]
    assert list(inspect.signature(live.LiveNode.run).parameters) == ["self"]
    assert method.args.vararg is None
    assert method.args.kwarg is None
