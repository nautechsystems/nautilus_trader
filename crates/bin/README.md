# Nautilus Bin

Live trading binaries for the Nautilus Trader ecosystem. Configuration is managed via `config.toml` and environment variables (`.env`).

## Binaries

| Binary | Description |
|--------|-------------|
| `strategy_runner` | Runs any registered strategy selected in `[runner]` of the config file |
| `recorder` | Market data recorder that writes live data to disk |

## Usage

```bash
# Run the strategy selected in [runner] of config.toml
cargo run --bin strategy_runner

# Run the market data recorder
cargo run --bin recorder
```

Both binaries load their configuration from `config.toml` in the crate root (override with `--config-path`). Set `exchange` to `"bybit"` or `"dydx"` and configure exchange-specific credentials via environment variables.

## Runtime strategy switching

The `strategy_runner` binary runs the strategy named in the `[runner]` section of `config.toml`. Switching strategies only requires editing the config file, no recompilation:

```toml
[runner]
strategy = "grid_mm"  # or "mmm"
```

Registered strategies:

| Name | Strategy |
|------|----------|
| `grid_mm` | Grid market-making strategy for perpetual futures |
| `mmm` | Mattia's market maker |

Release binaries can be built and run with:

```bash
cargo build --release
./target/release/strategy_runner
./target/release/recorder
```

## Building
use this tool https://github.com/cat-in-136/cargo-generate-rpm
