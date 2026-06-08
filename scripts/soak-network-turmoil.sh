#!/usr/bin/env bash
set -euo pipefail

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
  cat << 'EOF'
Run the nautilus-network Turmoil reconnect soak.

The soak sweeps deterministic Turmoil seeds. For each seed, it runs the
Tungstenite WebSocket backend, then the Sockudo WebSocket backend when
transport-sockudo is enabled.

Environment:
  NAUTILUS_TURMOIL_SOAK_START              First seed to run, default 0
  NAUTILUS_TURMOIL_SOAK_COUNT              Number of seeds to run, unset runs until stopped
  NAUTILUS_TURMOIL_SOAK_PROGRESS_INTERVAL  Log every N seeds per backend, default 100

Examples:
  scripts/soak-network-turmoil.sh
  env NAUTILUS_TURMOIL_SOAK_COUNT=100 scripts/soak-network-turmoil.sh
EOF
  exit 0
fi

echo "Running nautilus-network Turmoil soak"
echo "  start: ${NAUTILUS_TURMOIL_SOAK_START:-0}"
echo "  count: ${NAUTILUS_TURMOIL_SOAK_COUNT:-unbounded}"
echo "  progress interval: ${NAUTILUS_TURMOIL_SOAK_PROGRESS_INTERVAL:-100}"

cargo test -p nautilus-network --features turmoil,transport-sockudo \
  --test turmoil_websocket \
  test_turmoil_websocket_repeated_drops_backend_pair_soak \
  -- \
  --ignored \
  --nocapture \
  --test-threads=1
