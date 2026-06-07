#!/usr/bin/env bash
# Runs the network Turmoil clippy mix on non-Linux hosts.
set -euo pipefail

if [ "${NAUTILUS_FORCE_NETWORK_TURMOIL_CLIPPY:-0}" != "1" ]; then
  case "$(uname -s)" in
    Linux*)
      echo "Linux detected; skipping non-Linux network Turmoil clippy"
      exit 0
      ;;
  esac
fi

profile="${CARGO_CI_PROFILE:-nextest}"

cargo clippy -p nautilus-network --lib --tests \
  --features "python,turmoil" \
  --profile "$profile" -- -D warnings
