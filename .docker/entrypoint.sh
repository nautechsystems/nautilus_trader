#!/bin/bash
# entrypoint script for DockerfileUbuntu

echo "=== Nautilus Trader Development Environment ==="
echo "Rust version: $(rustc --version)"
echo "UV version: $(uv --version)"
echo "Working directory: $(pwd)"
echo

echo "=== Setting PyO3 environment ==="
export PYO3_PYTHON=/workspace/.venv/bin/python3
echo "PYO3_PYTHON: $PYO3_PYTHON"
echo

echo "=== Development environment ready! ==="
echo "You can now run for example:"
echo "  make install-debug                                            # Install nautilus in debug mode"
echo "  make cargo-test                                               # Test Rust code"
echo "  make pytest                                                   # Run Python tests"
echo "  uv run python -c \"import nautilus_trader.backtest.engine;\"    # Run a Python instruction"
echo

# If no command is provided, check if we have a TTY and start appropriate shell
if [ $# -eq 0 ]; then
  if [ -t 0 ]; then
    echo "Starting interactive shell..."
    exec bash
  else
    echo "No TTY detected. Use docker run -it for interactive mode."
    echo "Container ready for commands. Example:"
    echo "  docker run --rm -itv \"\$(pwd)\":/workspace nautilus-dev"
  fi
else
  exec "$@"
fi
