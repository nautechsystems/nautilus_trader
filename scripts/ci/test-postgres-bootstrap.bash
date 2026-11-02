#!/usr/bin/env bash

set -euo pipefail

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "Skipping PostgreSQL bootstrap tests: PostgreSQL integration tests only run on Linux."
  exit 0
fi

container="nautilus-postgres-preflight-$$"
postgres_directory=""
export POSTGRES_PASSWORD=pass

cleanup() {
  if [[ -n "$postgres_directory" ]]; then
    if [[ -f "$postgres_directory/data/postmaster.pid" ]]; then
      "$postgres_bin/pg_ctl" --pgdata "$postgres_directory/data" --mode fast --wait stop
    fi
    rm -rf "$postgres_directory"
  else
    docker rm --force "$container" > /dev/null 2>&1 || true
  fi
}

if command -v docker > /dev/null 2>&1; then
  trap cleanup EXIT
  postgres_image="public.ecr.aws/docker/library/postgres:18.6-alpine3.23@sha256:"
  postgres_image+="dec395d8c09c45609d6deb5c2388a07882d64fa59b1d98c1895792e085f57888"
  if ! docker image inspect "$postgres_image" > /dev/null 2>&1; then
    bash scripts/ci/docker-pull-retry.sh "$postgres_image" 3
  fi

  docker run \
    --detach \
    --name "$container" \
    --publish 127.0.0.1::5432 \
    --env POSTGRES_USER=postgres \
    --env "POSTGRES_PASSWORD=$POSTGRES_PASSWORD" \
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
  postgres_host=127.0.0.1
  psql_command=(docker exec --interactive "$container" psql)
else
  if ! command -v pg_config > /dev/null 2>&1; then
    echo "ERROR: Docker or native PostgreSQL server tools are required for PostgreSQL bootstrap tests." >&2
    exit 1
  fi
  postgres_bin="$(pg_config --bindir)"
  if [[ ! -x "$postgres_bin/initdb" || ! -x "$postgres_bin/pg_ctl" ]]; then
    echo "ERROR: PostgreSQL server tools are missing from $postgres_bin." >&2
    exit 1
  fi
  postgres_directory="$(mktemp -d /tmp/nautilus-postgres-preflight.XXXXXX)"
  trap cleanup EXIT
  "$postgres_bin/initdb" --pgdata "$postgres_directory/data" --username postgres \
    --encoding UTF8 --no-locale --auth-local scram-sha-256 --auth-host reject \
    --pwfile <(printf '%s\n' "$POSTGRES_PASSWORD") > /dev/null
  postgres_port=5432
  if ! "$postgres_bin/pg_ctl" --pgdata "$postgres_directory/data" \
    --log "$postgres_directory/postgres.log" --wait --timeout 30 \
    --options "-c listen_addresses='' -c unix_socket_directories='$postgres_directory' -p $postgres_port" start; then
    cat "$postgres_directory/postgres.log" >&2
    exit 1
  fi
  postgres_host="$postgres_directory"
  psql_command=(
    env -u PGHOSTADDR -u PGSERVICE PGPASSWORD="$POSTGRES_PASSWORD"
    "$postgres_bin/psql" --no-psqlrc --host "$postgres_host" --port "$postgres_port"
  )
  "${psql_command[@]}" --set ON_ERROR_STOP=1 --username postgres --dbname postgres \
    --command 'CREATE DATABASE nautilus' > /dev/null
fi

export POSTGRES_HOST="$postgres_host"
export POSTGRES_PORT="$postgres_port"
export POSTGRES_DATABASE=nautilus

for schema_file in types.sql tables.sql functions.sql partitions.sql; do
  "${psql_command[@]}" --quiet --set ON_ERROR_STOP=1 --username postgres --dbname nautilus \
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

POSTGRES_USERNAME=nautilus cargo nextest run --locked \
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

role_exists="$("${psql_command[@]}" \
  --tuples-only \
  --no-align \
  --username postgres \
  --dbname nautilus \
  --command "SELECT 1 FROM pg_roles WHERE rolname = 'nautilus'")"
if [[ "$role_exists" == "1" ]]; then
  echo "ERROR: PostgreSQL application role still exists after database drop." >&2
  exit 1
fi
