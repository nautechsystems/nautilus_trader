# nautilus-blockchain

## Scripts
You can run some example scripts and provide target RPC environment variables. These examples demonstrate how to connect to blockchain nodes and subscribe to various events.

You can configure the required environment variables in two ways:

1. **Using a `.env` file in the project root:**
   Create a file named `.env` in the project root directory with the following content:

   ```
   CHAIN=Ethereum
   RPC_WSS_URL=wss://mainnet.infura.io/ws/v3/YOUR_INFURA_API_KEY
   ```

2. **Providing variables directly in the command line:**

   ```
   CHAIN=Ethereum RPC_WSS_URL=wss://your-node-endpoint cargo run --bin live_blocks
   ```

### Watch live blocks

The script will connect to the specified blockchain and log information about each new block received.

```
Running `target/debug/live_blocks`
2025-04-25T14:54:41.394620000Z [INFO] TRADER-001.nautilus_blockchain::rpc::core: Subscribing to new blocks on chain Ethereum
2025-04-25T14:54:48.951608000Z [INFO] TRADER-001.nautilus_blockchain::data: Block(chain=Ethereum, number=22346765, timestamp=2025-04-25T14:54:47+00:00, hash=0x18a3c9f1e3eec06b45edc1f632565e5c23089dc4ad0892b00fda9e4ffcc9bf91)
2025-04-25T14:55:00.646992000Z [INFO] TRADER-001.nautilus_blockchain::data: Block(chain=Ethereum, number=22346766, timestamp=2025-04-25T14:54:59+00:00, hash=0x110436e41463daeacd1501fe53d38c310573abc136672a12054e1f33797ffeb9)
2025-04-25T14:55:14.369337000Z [INFO] TRADER-001.nautilus_blockchain::data: Block(chain=Ethereum, number=22346767, timestamp=2025-04-25T14:55:11+00:00, hash=0x54e7dbcfc14c058e22c70cbacabe4872e84bd6d3b976258f0d364ae99226b314)
^C2025-04-25T14:55:38.314022000Z [INFO] TRADER-001.live_blocks: Shutdown signal received, shutting down...

```
