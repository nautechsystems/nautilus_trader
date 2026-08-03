#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "Usage: $0 <event-name> <ref-name>" >&2
  exit 1
fi

event_name=$1
ref_name=$2

if [[ "$event_name" != "push" ]]; then
  echo "none"
  exit 0
fi

case "$ref_name" in
  develop)
    echo "development"
    ;;
  nightly)
    echo "nightly"
    ;;
  *)
    echo "none"
    ;;
esac
