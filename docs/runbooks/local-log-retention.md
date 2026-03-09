# Local Log Retention

Flux local smoke stacks keep logs under:

- `.run/tokenmm-stack/logs`
- `.run/equities-stack/logs`

The stack scripts now rotate a log before append when the current file exceeds a size budget and keep only a bounded number of rotated files.

## TokenMM

- `TOKENMM_LOCAL_LOG_MAX_MB`
- `TOKENMM_LOCAL_LOG_KEEP`

Defaults:

- max file size: `100 MB`
- rotated files kept per log: `5`

## Equities

- `EQUITIES_LOCAL_LOG_MAX_MB`
- `EQUITIES_LOCAL_LOG_KEEP`

Defaults:

- max file size: `100 MB`
- rotated files kept per log: `5`

## Guidance

- These controls are for local smoke only, not production.
- Production Flux services stay journal-first under `systemd`.
- Set the max size to `0` only if you explicitly want to disable size-triggered rotation while still pruning old rotated files.
