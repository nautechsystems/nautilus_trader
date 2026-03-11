from __future__ import annotations

import os
import sys
from collections.abc import Mapping
from collections.abc import Sequence
from dataclasses import dataclass
from typing import Any


if __name__ == "flux.runners.shared.surface_proxy":
    sys.modules.setdefault("nautilus_trader.flux.runners.shared.surface_proxy", sys.modules[__name__])
elif __name__ == "nautilus_trader.flux.runners.shared.surface_proxy":
    sys.modules.setdefault("flux.runners.shared.surface_proxy", sys.modules[__name__])


@dataclass(frozen=True, slots=True)
class SurfaceProxyDescriptor:
    surface: str
    base_paths: tuple[str, ...]
    backend_env_var: str
    api_prefixes: tuple[str, ...] = ()
    profile_names: tuple[str, ...] = ()


def _normalize_text(value: Any) -> str:
    if value is None:
        return ""
    return str(value).strip()


def _normalize_path(value: Any) -> str:
    text = _normalize_text(value) or "/"
    if text.startswith("/"):
        return text
    return f"/{text}"


def _matches_path_prefix(path: str, prefix: str) -> bool:
    normalized_prefix = _normalize_path(prefix)
    return path == normalized_prefix or path.startswith(f"{normalized_prefix}/")


def resolve_surface_backends(
    descriptors: Sequence[SurfaceProxyDescriptor],
    *,
    env: Mapping[str, str] | None = None,
) -> dict[str, str]:
    source = os.environ if env is None else env
    resolved: dict[str, str] = {}
    for descriptor in descriptors:
        backend_url = _normalize_text(source.get(descriptor.backend_env_var))
        if backend_url:
            resolved[descriptor.surface] = backend_url
    return resolved


def resolve_surface_proxy_descriptor(
    *,
    path: Any,
    profile: Any,
    descriptors: Sequence[SurfaceProxyDescriptor],
) -> SurfaceProxyDescriptor | None:
    normalized_path = _normalize_path(path)
    normalized_profile = _normalize_text(profile).lower()

    for descriptor in descriptors:
        if any(_matches_path_prefix(normalized_path, prefix) for prefix in descriptor.base_paths):
            return descriptor

    for descriptor in descriptors:
        if (
            normalized_profile
            and normalized_profile in {name.lower() for name in descriptor.profile_names}
            and any(_matches_path_prefix(normalized_path, prefix) for prefix in descriptor.api_prefixes)
        ):
            return descriptor

    return None


__all__ = [
    "SurfaceProxyDescriptor",
    "resolve_surface_backends",
    "resolve_surface_proxy_descriptor",
]
