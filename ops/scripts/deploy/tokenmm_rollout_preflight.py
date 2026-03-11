#!/usr/bin/env python3
"""
Thin wrapper for the TokenMM rollout preflight module.
"""

from __future__ import annotations

import sys
from pathlib import Path


def main() -> int:
    repo_root = Path(__file__).resolve().parents[3]
    root_text = str(repo_root)
    if root_text not in sys.path:
        sys.path.insert(0, root_text)

    from flux.runners.tokenmm.rollout_preflight import main as rollout_main

    return rollout_main()


if __name__ == "__main__":
    raise SystemExit(main())
