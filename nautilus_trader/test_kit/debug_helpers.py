"""
Helper function for debugging rust bindings from Jupyter notebooks.
"""

import json
import os
import sys

import debugpy  # noqa: T100

from nautilus_trader import PACKAGE_ROOT


def setup_debugging(vs_code_path=PACKAGE_ROOT.parent, enable_python_debugging=True):
    # By default the directory containing the .vscode folder is assumed to be
    # one folder above the root nautilus_trader folder
    if enable_python_debugging:
        debugpy.listen(5678)  # noqa: T100

    # Get current process info
    pid = os.getpid()
    python_path = sys.executable

    # Essential configurations for mixed debugging only
    config = {
        "version": "0.2.0",
        "configurations": [
            {
                "name": "Rust Debugger (for Jupyter)",
                "type": "lldb",
                "request": "attach",
                "program": python_path,
                "pid": pid,
                "sourceLanguages": ["rust"],
                "env": {
                    "RUST_BACKTRACE": "1",
                },
            },
            {
                "name": "Python Debugger (for Jupyter)",
                "type": "debugpy",
                "request": "attach",
                "connect": {
                    "host": "localhost",
                    "port": 5678,
                },
                "pathMappings": [
                    {
                        "localRoot": "${workspaceFolder}/nautilus_trader",
                        "remoteRoot": "${workspaceFolder}/nautilus_trader",
                    },
                ],
                "env": {
                    "RUST_BACKTRACE": "1",
                    "PYTHONPATH": "${workspaceFolder}/nautilus_trader",
                },
            },
        ],
        "compounds": [
            {
                "name": "Python + Rust Debugger (for Jupyter)",
                "configurations": [
                    "Python Debugger (for Jupyter)",
                    "Rust Debugger (for Jupyter)",
                ],
                "stopAll": True,
                "presentation": {
                    "hidden": False,
                    "group": "mixed",
                    "order": 2,
                },
            },
        ],
    }

    # Determine path
    launch_json_path = vs_code_path / ".vscode" / "launch.json"
    print(f"{launch_json_path=}")

    # Create .vscode directory if it doesn't exist
    launch_json_path.parent.mkdir(exist_ok=True)

    # Write the configuration
    with open(launch_json_path, "w") as f:
        json.dump(config, f, indent=4)

    print("✓ VS Code configuration updated")
    print(
        f"Created {len(config['configurations'])} configurations and {len(config['compounds'])} compound configurations",
    )
    print("1. In VS Code: Select 'Python + Rust Debugger (for Jupyter)' → Start Debugging (F5)")
