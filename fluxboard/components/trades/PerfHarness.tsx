import { useEffect, useMemo, useRef, useState } from 'react';
import { Button } from '../ui/button/Button';
import type { TradeRow } from '../../types';
import { colors, typography } from '@/lib/tokens';
import { TradesTable } from './TradesTable';

const PERF_ROW_COUNT = 50_000;
const EVENT_INTERVAL_MS = 15;

const coins = ['PLUME', 'ETH', 'BTC', 'SOL', 'ARB', 'MATIC', 'WSEI', 'USDC', 'USDT'];
const exchanges = ['bybit', 'bitget', 'rooster', 'sailor'];
const sides: Array<'buy' | 'sell'> = ['buy', 'sell'];

function createSyntheticRow(index: number): TradeRow {
  const coin = coins[index % coins.length];
  const exchange = exchanges[index % exchanges.length];
  const price = 10 + ((index % 40) * 0.25);
  const qty = Number((1 + (index % 15) * 0.1).toFixed(3));
  const rowId = `perf-${index}`;
  const timestamp = Date.now() - index;
  return {
    row_id: rowId,
    version: 1,
    seq: index,
    ts: timestamp,
    time: new Date(timestamp).toISOString(),
    coin,
    exchange,
    venue: index % 2 === 0 ? 'cex' : 'dex',
    symbol: `${coin}/USDT`,
    side: sides[index % sides.length],
    price,
    qty,
    mv: Number((price * qty).toFixed(4)),
    fee: Number((price * qty * 0.0005).toFixed(4)),
    exch_id: `tx_${rowId}`,
    trade_id: `trd_${rowId}`,
    signal_id: `sig_${index % 12}`,
    order_id: `ord_${rowId}`,
    decision: index % 5 === 0 ? 'simulated' : undefined,
    decision_timestamp: new Date(timestamp).toISOString(),
    gas_used: (index % 20) + 1,
    notes: 'Perf harness trade',
    explorer_url: 'https://example.com',
    placeholder: false,
  };
}

export function TradesPerfHarness({ onClose }: { onClose: () => void }) {
  const baseRows = useMemo(
    () => Array.from({ length: PERF_ROW_COUNT }, (_, idx) => createSyntheticRow(idx)),
    [],
  );
  const [rows, setRows] = useState(baseRows);
  const eventsRef = useRef(0);
  const [eventRate, setEventRate] = useState(0);
  const [fps, setFps] = useState(0);
  const fpsRef = useRef({ last: 0, count: 0 });

  useEffect(() => {
    if (typeof window === 'undefined') {
      return () => {};
    }
    const interval = window.setInterval(() => {
      setRows((prev) => {
        const next = prev.slice();
        const idx = Math.floor(Math.random() * next.length);
        const current = next[idx];
        const delta = (Math.random() - 0.5) * 0.75;
        const newPrice = Math.max(0.5, Number(current.price ?? 0) + delta);
        const newQty = Math.max(0.05, Number(current.qty ?? 1) + (Math.random() - 0.5) * 0.3);
        const updated: TradeRow = {
          ...current,
          price: Number(newPrice.toFixed(4)),
          qty: Number(newQty.toFixed(4)),
          mv: Number((newPrice * newQty).toFixed(4)),
          fee: Number((newPrice * newQty * 0.0005).toFixed(4)),
          seq: current.seq + 1,
          version: current.version + 1,
          ts: Date.now(),
          time: new Date().toISOString(),
          notes: current.notes,
        };
        next[idx] = updated;
        return next;
      });
      eventsRef.current += 1;
    }, EVENT_INTERVAL_MS);
    return () => {
      window.clearInterval(interval);
    };
  }, []);

  useEffect(() => {
    if (typeof window === 'undefined') {
      return () => {};
    }
    const rateTimer = window.setInterval(() => {
      setEventRate(eventsRef.current);
      eventsRef.current = 0;
    }, 1000);
    return () => {
      window.clearInterval(rateTimer);
    };
  }, []);

  useEffect(() => {
    if (typeof window === 'undefined') {
      return () => {};
    }
    let frame: number | null = null;
    const tick = (time: number) => {
      if (fpsRef.current.last === 0) {
        fpsRef.current.last = time;
      }
      fpsRef.current.count += 1;
      const delta = time - fpsRef.current.last;
      if (delta >= 1000) {
        setFps(Math.max(0, Math.round((fpsRef.current.count * 1000) / delta)));
        fpsRef.current.count = 0;
        fpsRef.current.last = time;
      }
      frame = window.requestAnimationFrame(tick);
    };
    frame = window.requestAnimationFrame(tick);
    return () => {
      if (frame !== null) {
        window.cancelAnimationFrame(frame);
      }
    };
  }, []);

  return (
    <div className="flex flex-col h-full min-h-0">
      <div
        className="flex items-center justify-between border-b px-4 py-2"
        style={{
          backgroundColor: colors.bg.surface,
          borderBottomColor: colors.border.DEFAULT,
          color: colors.text.secondary,
          fontSize: typography.fontSize.sm,
        }}
      >
        <div className="flex flex-col">
          <span className="font-mono uppercase" style={{ color: colors.text.primary }}>
            Perf Harness (50k rows)
          </span>
          <span style={{ color: colors.text.muted }}>
            Simulating ~{Math.round(1000 / EVENT_INTERVAL_MS)} event/s updates
          </span>
        </div>
        <div className="flex items-center gap-4" style={{ fontSize: typography.fontSize.sm }}>
          <span style={{ color: colors.text.secondary }}>FPS: {fps}</span>
          <span style={{ color: colors.text.secondary }}>Events: {eventRate}/s</span>
          <Button variant="ghost" size="xs" onClick={onClose}>
            Return to live view
          </Button>
        </div>
      </div>
      <div className="flex-1 min-h-0">
        <TradesTable
          trades={rows}
          sortDirection="ts_desc"
          onTimeSortChange={() => {}}
        />
      </div>
    </div>
  );
}
