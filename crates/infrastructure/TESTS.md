# Infrastructure integration Tests

This directory contains infrastructure integration tests that require external services.

## Service Requirements
All required services are defined in `.docker/docker-compose.yml`.

The integration tests require the following services to be running:

- PostgreSQL on `localhost:5432`
- Redis on `localhost:6379`

### Service Configuration

- **PostgreSQL**: Username `nautilus`, Password `pass`, Database `nautilus`
- **Redis**: Default configuration, no authentication
- **PgAdmin** (Optional): Available at `http://localhost:5051` (<admin@mail.com> / admin)

## Running Integration Test Services
Use the following make targets to manage the services:

### Initial Setup

```bash
make init-services  # Start containers and initialize database schema
```

### Managing Services

```bash
make stop-services   # Stop development services (preserves data)
make start-services  # Start development services (without reinitializing database)
make purge-services  # Remove everything including data volumes
```

### Typical Workflow

1. First time: `make init-services`
2. Stop when done: `make stop-services`
3. Resume work: `make start-services`
4. Clean slate: `make purge-services` then `make init-services`

## Running Tests
Once services are running (and Nautilus Trader installed by `uv` or `make`):

### Python Infrastructure Integration Tests

```bash
# Run all infrastructure tests
uv run --no-sync pytest tests/integration_tests/infrastructure/

# Run specific test file
uv run --no-sync pytest tests/integration_tests/infrastructure/test_cache_database_redis.py
uv run --no-sync pytest tests/integration_tests/infrastructure/test_cache_database_postgres.py
```

### Rust Infrastructure Integration Tests
The Rust integration tests are located in `crates/infrastructure/tests/` and require the same services.

```bash
# Run all Rust integration tests (includes Redis and PostgreSQL tests)
make cargo-test-crate-nautilus-infrastructure

# Using cargo nextest directly with the standard profile
# Run all infrastructure test with output visible for debugging
cargo nextest run --lib --no-fail-fast --cargo-profile nextest -p nautilus-infrastructure --features redis,postgres --no-capture

# Run only Redis integration tests
cargo nextest run --lib --no-fail-fast --cargo-profile nextest -p nautilus-infrastructure --features redis,postgres -E 'test(test_cache_redis)'

# Run only PostgreSQL integration tests
cargo nextest run --lib --no-fail-fast --cargo-profile nextest -p nautilus-infrastructure --features redis,postgres -E 'test(test_cache_postgres) or test(test_cache_database_postgres)'

```

**Note**: Both redis and postgres feature flags are in given examples to avoid rebuild.
Rust infrastructure integration tests are marked with `#[cfg(target_os = "linux")]` and will only run on Linux.
They use the `serial_test` crate to ensure tests that access the same database don't run concurrently.
