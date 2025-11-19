# Observability (logging and metrics)

This project uses `tracing` for structured logs. You can switch between pretty logs and JSON, and optionally export Prometheus metrics.

## JSON logs (example)
Add to your binary initialization (or set `RUST_LOG`):

```rust path=null start=null
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

fn init_tracing_json() {
    let fmt_layer = fmt::layer()
        .with_target(true)
        .with_file(true)
        .with_line_number(true)
        .json();
    let filter = EnvFilter::from_default_env();
    tracing_subscriber::registry().with(filter).with(fmt_layer).init();
}
```

Run with:

```bash path=null start=null
RUST_LOG=info ./your-binary
```

## Prometheus metrics (optional)
If you enable the metrics feature (to be wired per binary), you can expose an endpoint:

```rust path=null start=null
use std::net::SocketAddr;
use axum::{routing::get, Router};
use metrics_exporter_prometheus::PrometheusBuilder;

async fn serve_metrics(addr: SocketAddr) {
    let handle = PrometheusBuilder::new().install_recorder().unwrap();
    let app = Router::new().route("/metrics", get(move || async move { handle.render() }));
    axum::Server::bind(&addr).serve(app.into_make_service()).await.unwrap();
}
```

Then run a tokio task for `serve_metrics` (e.g., `0.0.0.0:9090`).

## Tips
- Prefer `tracing::instrument` on hot paths to correlate events.
- Use `RUST_LOG=nautilus_*=debug,info` to scope verbosity.
- For per-test logs, use `RUST_LOG=debug cargo nextest run -E 'test(<name>)'`.
