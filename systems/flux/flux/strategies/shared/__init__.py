import sys

from flux.strategies.shared import quote_stack
from flux.strategies.shared.publisher_common import build_role_map_payload
from flux.strategies.shared.quote_stack import ActiveStackLevel
from flux.strategies.shared.quote_stack import DesiredStackLevel
from flux.strategies.shared.quote_health import QuoteHealth
from flux.strategies.shared.quote_health import evaluate_quote_health
from flux.strategies.shared.quote_snapshot import build_quote_snapshot_payload
from flux.strategies.shared.quote_stack import StackAction
from flux.strategies.shared.quote_stack import StackPlan
from flux.strategies.shared.quote_stack import StackPlanDiagnostics
from flux.strategies.shared.quote_stack import plan_quote_stack
from flux.strategies.shared.quote_stack import plan_side_deque_actions

if __name__ == "flux.strategies.shared":
    sys.modules.setdefault("nautilus_trader.flux.strategies.shared", sys.modules[__name__])
elif __name__ == "nautilus_trader.flux.strategies.shared":
    sys.modules.setdefault("flux.strategies.shared", sys.modules[__name__])


__all__ = [
    "build_quote_snapshot_payload",
    "build_role_map_payload",
    "QuoteHealth",
    "evaluate_quote_health",
    "quote_stack",
    "ActiveStackLevel",
    "DesiredStackLevel",
    "StackAction",
    "StackPlan",
    "StackPlanDiagnostics",
    "plan_quote_stack",
    "plan_side_deque_actions",
]
