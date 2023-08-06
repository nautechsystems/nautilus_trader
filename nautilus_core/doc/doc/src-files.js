var srcIndex = JSON.parse('{\
"nautilus_backtest":["",[],["engine.rs","lib.rs"]],\
"nautilus_common":["",[],["clock.rs","clock_api.rs","enums.rs","lib.rs","logging.rs","logging_api.rs","msgbus.rs","testing.rs","timer.rs","timer_api.rs"]],\
"nautilus_core":["",[],["correctness.rs","cvec.rs","datetime.rs","lib.rs","parsing.rs","serialization.rs","string.rs","time.rs","uuid.rs"]],\
"nautilus_indicators":["",[],["ema.rs","lib.rs"]],\
"nautilus_model":["",[["data",[],["bar.rs","bar_api.rs","delta.rs","delta_api.rs","mod.rs","order.rs","order_api.rs","quote.rs","quote_api.rs","trade.rs","trade_api.rs"]],["events",[],["mod.rs","order.rs","order_api.rs","position.rs"]],["identifiers",[],["account_id.rs","client_id.rs","client_order_id.rs","component_id.rs","exec_algorithm_id.rs","instrument_id.rs","macros.rs","mod.rs","order_list_id.rs","position_id.rs","strategy_id.rs","symbol.rs","trade_id.rs","trader_id.rs","venue.rs","venue_order_id.rs"]],["instruments",[],["crypto_future.rs","crypto_perpetual.rs","currency_pair.rs","equity.rs","futures_contract.rs","mod.rs","options_contract.rs","synthetic.rs","synthetic_api.rs"]],["orderbook",[],["book.rs","book_api.rs","ladder.rs","level.rs","level_api.rs","mod.rs"]],["orders",[],["base.rs","limit.rs","limit_if_touched.rs","market.rs","market_if_touched.rs","market_to_limit.rs","mod.rs","stop_limit.rs","trailing_stop_limit.rs","trailing_stop_market.rs"]],["types",[],["balance.rs","currency.rs","fixed.rs","mod.rs","money.rs","price.rs","quantity.rs"]]],["currencies.rs","enums.rs","lib.rs","macros.rs","position.rs"]],\
"nautilus_network":["",[],["http.rs","lib.rs","socket.rs","websocket.rs"]],\
"nautilus_persistence":["",[["arrow",[],["bar.rs","delta.rs","mod.rs","quote.rs","trade.rs"]],["backend",[],["mod.rs","session.rs","transformer.rs"]],["wranglers",[],["bar.rs","delta.rs","mod.rs","quote.rs","trade.rs"]]],["kmerge_batch.rs","lib.rs"]],\
"nautilus_pyo3":["",[],["lib.rs"]],\
"tokio_tungstenite":["",[],["compat.rs","connect.rs","handshake.rs","lib.rs","stream.rs","tls.rs"]]\
}');
createSrcSidebar();
