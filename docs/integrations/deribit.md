# Deribit

Founded in 2016, Deribit is a cryptocurrency derivatives exchange specializing in Bitcoin and
Ethereum options and futures. It is one of the largest crypto options exchanges by volume.
This integration supports live market data ingest and order execution on Deribit.

:::warning
This integration is currently under construction and not yet ready for use.
:::

## Overview

This adapter is implemented in Rust, with optional Python bindings use in Python-based workflows.
Deribit uses JSON-RPC 2.0 over both HTTP and WebSocket transports (rather than REST).
WebSocket is preferred for subscriptions and real-time data.

The official Deribit API reference can be found at <https://docs.deribit.com/v2/>.
