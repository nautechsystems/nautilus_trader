#!/usr/bin/env bash
set -euo pipefail

if [ "${PLAN_RESULT:-}" != "success" ] ||
  [ "${PRE_COMMIT_RESULT:-}" != "success" ] ||
  [ "${SCRIPT_TESTS_RESULT:-}" != "success" ]; then
  echo "::error::Python check prerequisites did not succeed"
  exit 1
fi

for result in "${BUILD_RESULT:-}" "${BUILD_PYTHON314_RESULT:-}" "${BUILD_PYTHON315_RESULT:-}"; do
  case "${RUN_TESTS:-}:${result}" in
    true:success | false:skipped) ;;
    *)
      echo "::error::Unexpected Python build result: run-tests=${RUN_TESTS:-unset}, build=${result:-unset}"
      exit 1
      ;;
  esac
done

if [ "$RUN_TESTS" = true ]; then
  echo "Python 3.13, 3.14, and 3.15 builds, tests, and generated file checks passed"
else
  echo "Python build and tests intentionally skipped"
fi
