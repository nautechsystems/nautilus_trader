# Hyperliquid Testnet Order Placer - Root Cause Analysis

## Issue Summary

The order placer script connects successfully to Hyperliquid testnet but **no orders are placed** because the strategy never receives quote tick data.

## Root Cause

### Missing Implementation in Rust Data Client

The Hyperliquid data client (`crates/adapters/hyperliquid/src/data/mod.rs`) is **missing a critical message processing loop**.

#### What's Implemented ✅

1. WebSocket connection establishment
2. BBO subscription requests sent to Hyperliquid
3. Quote tick parsing functions exist (`websocket/parse.rs`)
4. All infrastructure for instruments and client management

#### What's Missing ❌

**No background task to process incoming WebSocket messages**

The data client lacks the message processing loop that should:

1. Continuously read from WebSocket (`next_event()`)
2. Parse BBO messages into `QuoteTick` objects
3. Emit `QuoteTick` data events to the data engine

### Comparison with Working Adapters

#### OKX Adapter (Working Example)

```rust
// crates/adapters/okx/src/data/mod.rs lines 307-350
fn spawn_stream_task(
    &mut self,
    stream: impl futures_util::Stream<Item = NautilusWsMessage> + Send + 'static,
) -> anyhow::Result<()> {
    let data_sender = self.data_sender.clone();
    let instruments = self.instruments.clone();
    let cancellation = self.cancellation_token.clone();

    let handle = tokio::spawn(async move {
        tokio::pin!(stream);

        loop {
            tokio::select! {
                maybe_msg = stream.next() => {
                    match maybe_msg {
                        Some(msg) => {
                            Self::handle_ws_message(msg, &data_sender, &instruments);
                        }
                        None => break,
                    }
                }
                _ = cancellation.cancelled() => break,
            }
        }
    });

    self.tasks.push(handle);
    Ok(())
}
```

#### Hyperliquid Adapter (Missing)

```rust
// crates/adapters/hyperliquid/src/data/mod.rs
async fn spawn_ws(&mut self) -> anyhow::Result<()> {
    self.ws_client
        .ensure_connected()
        .await
        .context("Failed to connect to Hyperliquid WebSocket")?;

    // ❌ MISSING: No message processing loop spawned here
    // ❌ MISSING: No task to call ws_client.next_event() in a loop
    // ❌ MISSING: No parsing of BBO messages to QuoteTicks
    // ❌ MISSING: No data event emission

    Ok(())
}
```

## Evidence from Logs

### Script Output

```
2025-10-14T16:28:28.164221000Z [INFO] ORDER-PLACER-001.OrderPlacer: [CMD]--> SubscribeQuoteTicks(...)
2025-10-14T16:28:28.164258000Z [INFO] ORDER-PLACER-001.OrderPlacer: RUNNING
```

The strategy:

- ✅ Starts successfully
- ✅ Subscribes to quote ticks
- ✅ WebSocket connects
- ❌ **Never receives `on_quote_tick()` callbacks**
- ❌ **Never places any orders**

### WebSocket Connection Success

```
2025-10-14T16:28:28.162096000Z [INFO] Hyperliquid WebSocket connected: wss://api.hyperliquid-testnet.xyz/ws
2025-10-14T16:28:28.162174000Z [INFO] DataClient-HYPERLIQUID: Connected to WebSocket
```

The WebSocket connection is established, but messages are not being processed.

## Required Fix

### Implementation Needed in `crates/adapters/hyperliquid/src/data/mod.rs`

Add a message processing loop similar to OKX:

```rust
async fn spawn_ws(&mut self) -> anyhow::Result<()> {
    self.ws_client
        .ensure_connected()
        .await
        .context("Failed to connect to Hyperliquid WebSocket")?;

    // NEW: Spawn background task to process WebSocket messages
    let ws_client = self.ws_client.clone();
    let data_sender = self.data_sender.clone();
    let instruments = self.instruments.clone();
    let cancellation = self.cancellation_token.clone();

    let handle = tokio::spawn(async move {
        loop {
            tokio::select! {
                event = ws_client.next_event() => {
                    match event {
                        Some(HyperliquidWsMessage::Bbo { data }) => {
                            // Parse BBO to QuoteTick
                            if let Some(instrument) = get_instrument_for_coin(&data.coin, &instruments) {
                                match parse_quote_tick_from_bbo(&data, &instrument) {
                                    Ok(quote_tick) => {
                                        send_data(&data_sender, Data::Quote(quote_tick));
                                    }
                                    Err(e) => {
                                        tracing::warn!("Failed to parse BBO: {}", e);
                                    }
                                }
                            }
                        }
                        Some(_) => {
                            // Handle other message types
                        }
                        None => {
                            tracing::debug!("WebSocket stream ended");
                            break;
                        }
                    }
                }
                _ = cancellation.cancelled() => {
                    tracing::debug!("WebSocket task cancelled");
                    break;
                }
            }
        }
    });

    self.tasks.push(handle);
    Ok(())
}
```

## Verification Steps

After implementing the fix:

1. Run the script again:

   ```bash
   python hyperliquid_testnet_order_placer.py --asset BTC --side buy --size 0.001
   ```

2. Expected log output should include:

   ```
   [INFO] OrderPlacer: Market Data: Bid=$XXXXX, Ask=$XXXXX
   [INFO] OrderPlacer: Mid Price: $XXXXX
   [INFO] OrderPlacer: Placing order: O-...
   [INFO] ORDER ACCEPTED: ...
   ```

3. Verify on Hyperliquid testnet UI:
   - <https://app.hyperliquid-testnet.xyz/portfolio>
   - Order should appear in "Open Orders" tab

## Additional Notes

### Existing Parse Functions

The parsing infrastructure already exists:

- `crates/adapters/hyperliquid/src/websocket/parse.rs::parse_quote_tick_from_bbo()` (line 180)
- All necessary types and conversions are implemented
- Just needs to be wired up in the message processing loop

### Message Types Supported

From `websocket/messages.rs`:

- `Bbo { data }` - Best bid/offer (needs processing for QuoteTicks)
- `Trades { data }` - Trade ticks
- `L2Book { data }` - Order book deltas
- `Candle { data }` - Bar data
- Others for execution/user events

### Python vs Rust Implementation

- Python adapter (`nautilus_trader/adapters/hyperliquid/data.py`) - delegates to Rust
- Rust implementation must be complete for Python to work
- This is a Rust-level infrastructure issue, not a Python configuration problem

## Conclusion

**The Hyperliquid data client establishes connections correctly but lacks the critical message processing loop to convert WebSocket events into Nautilus data objects.** This is why strategies never receive market data and cannot place orders based on live prices.

The fix requires implementing the message processing loop in the Rust data client, following the pattern used successfully in other adapters like OKX and Bybit.
