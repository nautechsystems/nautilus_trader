import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';

import { OrderViewL1Widget } from './OrderViewL1Widget';

const context = {
  maker: { exchange: 'bybit_linear', symbol: 'BTC_USDT' },
  hedge: { exchange: 'binance_spot', symbol: 'BTC_USDT' },
};

const status = {
  md_ok: true,
  maker_state_ok: true,
  events_ok: true,
  last_md_ts_ms: 1_000,
  last_state_ts_ms: 1_000,
  notes: [],
};

describe('OrderViewL1Widget', () => {
  it('shows missing status for legs with no BBO row', () => {
    render(
      <OrderViewL1Widget
        bbo={{
          maker: { bid: 100, ask: 101, mid: 100.5, ts_ms: 1_000 },
        }}
        l2={null}
        openOrders={[]}
        context={context}
        status={status}
        nowMs={1_100}
        staleThresholdMs={3_000}
        showBids
        showAsks
        focus={{ orderKey: null, eventKey: null, side: null, price: null }}
      />
    );

    expect(screen.getByText('live (100ms)')).toBeInTheDocument();
    expect(screen.getByText('missing (--)')).toBeInTheDocument();
    expect(screen.getByText('No L2 ladder data')).toBeInTheDocument();
  });

  it('marks stale rows using per-row timestamp age', () => {
    render(
      <OrderViewL1Widget
        bbo={{
          maker: { bid: 100, ask: 101, mid: 100.5, ts_ms: 1_000 },
          hedge: { bid: 99, ask: 102, mid: 100.5, ts_ms: 1_000 },
        }}
        l2={null}
        openOrders={[]}
        context={context}
        status={status}
        nowMs={5_000}
        staleThresholdMs={3_000}
        showBids
        showAsks
        focus={{ orderKey: null, eventKey: null, side: null, price: null }}
      />
    );

    const staleBadges = screen.getAllByText('stale (4.0s)');
    expect(staleBadges).toHaveLength(2);
  });

  it('renders top-n ladder rows with spread, depth bars, and our-order overlays', () => {
    render(
      <OrderViewL1Widget
        bbo={{
          maker: { bid: 30000, ask: 30001, mid: 30000.5, ts_ms: 1_000 },
        }}
        l2={{
          bids: [
            { px: '30000', qty: '2.0', size: 10 },
            { px: '29999', qty: '1.0', size: 5 },
          ],
          asks: [
            { px: '30001', qty: '2.5', size: 8 },
            { px: '30002', qty: '4.0', size: 4 },
          ],
          top_n: 2,
          spread_abs: 1,
          spread_bps: 3.33,
        }}
        openOrders={[
          {
            order_row_id: 'maker:bid:1:cl-1',
            leg: 'maker',
            side: 'bid',
            level: 1,
            px: '30000',
            rem_qty: '0.6',
            client_order_id: 'cl-1',
          },
          {
            order_row_id: 'maker:bid:2:cl-2',
            leg: 'maker',
            side: 'bid',
            level: 2,
            px: '30000',
            rem_qty: '0.4',
            client_order_id: 'cl-2',
          },
          {
            order_row_id: 'maker:ask:2:cl-3',
            leg: 'maker',
            side: 'ask',
            level: 2,
            px: '30002',
            rem_qty: '1.2',
            client_order_id: 'cl-3',
          },
        ]}
        context={context}
        status={status}
        nowMs={1_100}
        staleThresholdMs={3_000}
        showBids
        showAsks
        focus={{ orderKey: null, eventKey: null, side: null, price: null }}
      />
    );

    expect(screen.getByTestId('order-view-ladder-spread-row')).toHaveTextContent(
      'Spread (top 2)'
    );
    expect(screen.getByTestId('order-view-ladder-spread-row')).toHaveTextContent(
      '1.000000 (3.33 bps)'
    );

    expect(screen.getAllByText('BEST')).toHaveLength(2);
    expect(screen.getByTestId('order-view-ladder-our-bid-0')).toHaveTextContent('2 / 1.0000');
    expect(screen.getByTestId('order-view-ladder-our-ask-1')).toHaveTextContent('1 / 1.2000');

    expect(screen.getByTestId('order-view-ladder-depth-bid-0')).toHaveStyle({ width: '100.00%' });
    expect(screen.getByTestId('order-view-ladder-depth-ask-0')).toHaveStyle({ width: '80.00%' });
  });

  it('highlights focused ladder level and dims unrelated rows', () => {
    render(
      <OrderViewL1Widget
        bbo={{
          maker: { bid: 30000, ask: 30001, mid: 30000.5, ts_ms: 1_000 },
        }}
        l2={{
          bids: [
            { px: '30000', qty: '2.0', size: 10 },
            { px: '29999', qty: '1.0', size: 5 },
          ],
          asks: [
            { px: '30001', qty: '2.5', size: 8 },
            { px: '30002', qty: '4.0', size: 4 },
          ],
          top_n: 2,
          spread_abs: 1,
          spread_bps: 3.33,
        }}
        openOrders={[]}
        context={context}
        status={status}
        nowMs={1_100}
        staleThresholdMs={3_000}
        showBids
        showAsks
        focus={{ orderKey: null, eventKey: null, side: 'bid', price: 30000 }}
      />
    );

    expect(screen.getByTestId('order-view-ladder-row-bid-0')).toHaveAttribute('data-focus', 'focused');
    expect(screen.getByTestId('order-view-ladder-row-ask-0')).toHaveAttribute('data-focus', 'dimmed');
  });
});
