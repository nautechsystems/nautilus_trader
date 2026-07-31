# Nautilus Bin

Live trading binaries for the Nautilus Trader ecosystem. Configuration is managed via `config.toml` and environment variables (`.env`).

## Binaries

| Binary | Description |
|--------|-------------|
| `grid_mm` | Grid market-making strategy for perpetual futures |
| `recorder` | Market data recorder that writes live data to disk |

## Usage

```bash
# Run the grid market maker
cargo run --bin grid_mm

# Run the market data recorder
cargo run --bin recorder
```

Both binaries load their configuration from `config.toml` in the crate root. Set `exchange` to `"bybit"` or `"dydx"` and configure exchange-specific credentials via environment variables.

Release binaries can be built and run with:

```bash
cargo build --release
./target/release/grid_mm
./target/release/recorder
```

## Building
use this tool https://github.com/cat-in-136/cargo-generate-rpm