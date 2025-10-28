# Hyperliquid

:::warning
The Hyperliquid integration is still under active development.
:::

## Configuration

### Data client configuration options

| Option                   | Default | Description |
|--------------------------|---------|-------------|
| `base_url_http`          | `None`  | Override for the REST base URL. |
| `base_url_ws`            | `None`  | Override for the WebSocket base URL. |
| `testnet`                | `False` | Connect to the Hyperliquid testnet when `True`. |
| `http_timeout_secs`      | `10`    | Timeout (seconds) applied to REST calls. |

### Execution client configuration options

| Option                   | Default | Description |
|--------------------------|---------|-------------|
| `private_key`            | `None`  | EVM private key; loaded from `HYPERLIQUID_PK` when omitted. |
| `vault_address`          | `None`  | Vault address for delegated trading; loaded from `HYPERLIQUID_VAULT` when omitted. |
| `base_url_http`          | `None`  | Override for the REST base URL. |
| `base_url_ws`            | `None`  | Override for the WebSocket base URL. |
| `testnet`                | `False` | Connect to the Hyperliquid testnet when `True`. |
| `max_retries`            | `None`  | Maximum retry attempts for order submission/cancel/modify calls. |
| `retry_delay_initial_ms` | `None`  | Initial delay (milliseconds) between retries. |
| `retry_delay_max_ms`     | `None`  | Maximum delay (milliseconds) between retries. |
| `http_timeout_secs`      | `10`    | Timeout (seconds) applied to REST calls. |
