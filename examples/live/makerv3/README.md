# MakerV3 example config

This directory is for non-production MakerV3 example configuration only.

## Scope

- `config/makerv3.toml` is the example/dev config.
- Production configs live under `deploy/tokenmm/`.
- Production runner entrypoints live under `nautilus_trader/flux/runners/tokenmm/`.

## Local example usage

Run the package-owned runners against the example config. The package entrypoints require an explicit `--config`:

```bash
python -m nautilus_trader.flux.runners.tokenmm.run_node \
  --config examples/live/makerv3/config/makerv3.toml \
  --mode paper

python -m nautilus_trader.flux.runners.tokenmm.run_bridge \
  --config examples/live/makerv3/config/makerv3.toml \
  --mode paper

python -m nautilus_trader.flux.runners.tokenmm.run_api \
  --config examples/live/makerv3/config/makerv3.toml \
  --mode paper \
  --host 127.0.0.1 \
  --port 5022
```

## Production

Production deploy docs default to paper/no-exec smoke first.

- TokenMM multi-node deployment: `deploy/tokenmm/README.md`
- TokenMM UI serving runbook: `docs/fluxboard/tokenmm_runbook.md`
