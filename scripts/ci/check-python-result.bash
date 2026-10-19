#!/usr/bin/env bash
set -euo pipefail

if [ "${PLAN_RESULT:-}" != "success" ] ||
  [ "${PRE_COMMIT_RESULT:-}" != "success" ] ||
  [ "${SCRIPT_TESTS_RESULT:-}" != "success" ]; then
  echo "::error::Python check prerequisites did not succeed"
  exit 1
fi

case "${RUN_TESTS:-}:${BUILD_RESULT:-}" in
  true:success)
    echo "Python 3.13 build, tests, and generated file checks passed"
    ;;
  false:skipped)
    echo "Python build and tests intentionally skipped"
    ;;
  *)
    echo "::error::Unexpected Python build result: run-tests=${RUN_TESTS:-unset}, build=${BUILD_RESULT:-unset}"
    exit 1
    ;;
esac
