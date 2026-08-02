#!/usr/bin/env bash

set -euo pipefail

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "Skipping PostgreSQL bootstrap tests: PostgreSQL integration tests only run on Linux."
  exit 0
fi

if ! command -v docker > /dev/null 2>&1; then
  echo "ERROR: Docker is required for PostgreSQL bootstrap tests." >&2
  exit 1
fi

postgres_image="public.ecr.aws/docker/library/postgres:16.4-alpine@sha256:"
postgres_image+="5660c2cbfea50c7a9127d17dc4e48543eedd3d7a41a595a2dfa572471e37e64c"
container="nautilus-postgres-preflight-$$"

cleanup() {
  docker rm --force "$container" > /dev/null 2>&1 || true
}
trap cleanup EXIT

if ! docker image inspect "$postgres_image" > /dev/null 2>&1; then
  bash scripts/ci/docker-pull-retry.sh "$postgres_image" 3
fi

docker run \
  --detach \
  --name "$container" \
  --publish 127.0.0.1::5432 \
  --env POSTGRES_USER=postgres \
  --env POSTGRES_PASSWORD=pass \
  --env POSTGRES_DB=nautilus \
  "$postgres_image" > /dev/null

port_mapping="$(docker port "$container" 5432/tcp)"
postgres_port="${port_mapping##*:}"
case "$postgres_port" in
  '' | *[!0-9]*)
    echo "ERROR: Could not determine the PostgreSQL container port from: $port_mapping" >&2
    exit 1
    ;;
esac

attempt=1
until docker exec "$container" pg_isready \
  --host 127.0.0.1 \
  --port 5432 \
  --username postgres \
  --dbname nautilus > /dev/null 2>&1; do
  if [[ "$attempt" -ge 30 ]]; then
    docker logs "$container" >&2
    echo "ERROR: PostgreSQL did not become ready within 30 seconds." >&2
    exit 1
  fi
  attempt=$((attempt + 1))
  sleep 1
done

export POSTGRES_HOST=127.0.0.1
export POSTGRES_PORT="$postgres_port"
export POSTGRES_PASSWORD=pass
export POSTGRES_DATABASE=nautilus

for schema_file in types.sql tables.sql functions.sql partitions.sql; do
  docker exec --interactive "$container" \
    psql --quiet --set ON_ERROR_STOP=1 --username postgres --dbname nautilus \
    < "schema/sql/$schema_file"
done

POSTGRES_USERNAME=postgres cargo run \
  --locked \
  --package nautilus-cli \
  --bin nautilus \
  --profile "${CARGO_CI_PROFILE:-nextest}" \
  -- database init --schema "$PWD/schema/sql"

test_filter='test(test_init_postgres_skips_existing_objects_on_re_run)'
test_filter+=' or test(test_postgres_application_role_owns_schema_objects)'
test_filter+=' or test(test_order_column_migration_converts_legacy_floats_without_rounding)'
test_features="${POSTGRES_TEST_FEATURES:?POSTGRES_TEST_FEATURES must be set}"
if [[ "${NEXTEST_VERBOSE:-false}" == "true" ]]; then
  nextest_output_args=(--verbose)
else
  nextest_output_args=(--status-level fail --final-status-level flaky)
fi

POSTGRES_USERNAME=nautilus cargo nextest run \
  --workspace \
  --lib \
  --tests \
  --features "$test_features" \
  --profile "${NEXTEST_PROFILE:-default}" \
  --cargo-profile "${CARGO_CI_PROFILE:-nextest}" \
  "${nextest_output_args[@]}" \
  -E "$test_filter"

POSTGRES_USERNAME=postgres cargo run \
  --locked \
  --package nautilus-cli \
  --bin nautilus \
  --profile "${CARGO_CI_PROFILE:-nextest}" \
  -- database drop

role_exists="$(docker exec "$container" psql \
  --tuples-only \
  --no-align \
  --username postgres \
  --dbname nautilus \
  --command "SELECT 1 FROM pg_roles WHERE rolname = 'nautilus'")"
if [[ "$role_exists" == "1" ]]; then
  echo "ERROR: PostgreSQL application role still exists after database drop." >&2
  exit 1
fi
