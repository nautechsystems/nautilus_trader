# Docker services

## Postgres (local testing)

Postgres integration tests run on Linux when a Postgres instance is available.

### Start Postgres and init schema

From the repo root:

```bash
make init-services
```

This starts the Postgres container (from this `docker-compose.yml`), waits for it, and applies the schema (`schema/sql/types.sql`, `tables.sql`, `functions.sql`, `partitions.sql`).

Credentials (default): user `nautilus`, password `pass`, database `nautilus`, port `5432`.

### Run Postgres tests

**Python:**

```bash
make test-postgres
```

Requires `make init-services` (or at least `make start-services` then `make init-db`) to have been run first.

**Rust:**

```bash
POSTGRES_HOST=localhost POSTGRES_PORT=5432 POSTGRES_USERNAME=nautilus POSTGRES_PASSWORD=pass POSTGRES_DATABASE=nautilus \
  cargo test -p nautilus-infrastructure --features postgres -- --test-threads=1
```

### Start Postgres only (no schema)

```bash
docker compose -f .docker/docker-compose.yml up -d postgres
```

Then from repo root: `make init-db` to apply the schema.

### Stop / purge

- `make stop-services` — stop containers (data preserved).
- `make purge-services` — stop and remove volumes.
