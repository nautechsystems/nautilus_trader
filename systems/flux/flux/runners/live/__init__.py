import sys

_CURRENT_MODULE = sys.modules[__name__]

if __name__ == "flux.runners.live":
    sys.modules["nautilus_trader.flux.runners.live"] = _CURRENT_MODULE
elif __name__ == "nautilus_trader.flux.runners.live":
    sys.modules["flux.runners.live"] = _CURRENT_MODULE

runners_pkg = sys.modules.get("flux.runners")
if runners_pkg is not None:
    setattr(runners_pkg, "live", _CURRENT_MODULE)

compat_runners_pkg = sys.modules.get("nautilus_trader.flux.runners")
if compat_runners_pkg is not None:
    setattr(compat_runners_pkg, "live", _CURRENT_MODULE)

__all__ = ["ResolvedStrategyVenues", "resolve_strategy_venues"]


def __getattr__(name: str):
    if name in __all__:
        from flux.runners.live.venues import ResolvedStrategyVenues
        from flux.runners.live.venues import resolve_strategy_venues

        exports = {
            "ResolvedStrategyVenues": ResolvedStrategyVenues,
            "resolve_strategy_venues": resolve_strategy_venues,
        }
        return exports[name]
    raise AttributeError(name)
