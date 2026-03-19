import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import EquitiesArbSignalTable from '@/components/domain/signal/EquitiesArbSignalTable';
import type { SignalStrategy } from '@/types';

function buildEquitiesStrategy(
  id: string,
  strategyFamily: 'equities_maker' | 'equities_taker',
  label: 'Maker' | 'Taker',
): SignalStrategy {
  return {
    id,
    strategy_family: strategyFamily,
    running: true,
    tradeable: true,
    blocked: false,
    balances_ok: true,
    params: { bot_on: '1', qty: '1' },
    meta: {
      chain: 'equities',
      strategy_groups: 'equities',
      strategy_family: strategyFamily,
      class: strategyFamily,
      param_set: strategyFamily,
      base_asset: 'AAPL',
      quote_asset: 'USD',
    },
    maker_role_map: {
      maker_leg: 'maker',
      hedge_leg: 'hedge-blueocean',
      ref_leg: 'hedge',
    },
    legs_order: ['maker', 'hedge', 'hedge-blueocean'],
    legs: {
      maker: {
        exchange: 'hyperliquid',
        instrument_id: 'xyz:AAPL-USD-PERP.HYPERLIQUID',
        update_ts_ms: 1_700_000_000_000,
      },
      hedge: {
        exchange: 'ibkr',
        instrument_id: 'AAPL.NASDAQ',
        update_ts_ms: 1_700_000_000_500,
      },
      'hedge-blueocean': {
        exchange: 'ibkr',
        instrument_id: 'AAPL.BLUEOCEAN',
        update_ts_ms: 1_700_000_000_550,
      },
    },
    equities_arb: {
      operator: {
        execution_mode: strategyFamily === 'equities_taker' ? 'take_take' : 'maker_hedge',
        behavior: strategyFamily === 'equities_taker' ? 'take_take' : 'maker',
        hedge_policy: {
          route: 'SMART',
          time_in_force: 'DAY',
          outside_rth: true,
          include_overnight: true,
          cancel_after_ms: 5000,
        },
        fee_assumptions: {
          ibkr_fee_plan: 'tiered',
          ibkr_fee_min_usd: 0.35,
          hl_taker_fee_bps: 4.5,
          hl_maker_fee_bps: 0.25,
          assumed_hedge_fee_bps: 1.0,
        },
      },
      quote_snapshot: {
        ts_ms: 1_700_000_000_500,
        mid_spread_bps: strategyFamily === 'equities_taker' ? 1.5 : 2.0,
        arb_bid_spread_bps: 14.0,
        arb_ask_spread_bps: -11.0,
        effective_spread_bps: 6.5,
        quoted_spread_bps: 8.0,
        expected_maker_fee_bps: 0.25,
        assumed_hedge_fee_bps: 1.0,
        hedge_ready: true,
        hedge_route: 'BLUEOCEAN',
        fee_snapshot_age_s: 9,
        hedge_latency_ms: 45,
        hedge_slippage_bps_vs_mid: 1.5,
        maker_leg: {
          venue: 'HYPERLIQUID',
          instrument_id: 'xyz:AAPL-USD-PERP.HYPERLIQUID',
          feed_state: 'ok',
          quote_state: 'fresh',
        },
        hedge_leg: {
          venue: 'IBKR',
          instrument_id: 'AAPL.BLUEOCEAN',
          route: 'BLUEOCEAN',
          feed_state: 'ok',
          quote_state: 'fresh',
        },
        ref_leg: {
          venue: 'IBKR',
          instrument_id: 'AAPL.NASDAQ',
          feed_state: 'ok',
          quote_state: 'fresh',
        },
      },
    },
    state: {
      ts_ms: 1_700_000_000_500,
      state: label.toLowerCase(),
    },
  };
}

describe('EquitiesArbSignalTable', () => {
  it('renders one shared equities table with visible variant labels sorted by symbol then variant', () => {
    render(
      <EquitiesArbSignalTable
        rows={[
          buildEquitiesStrategy('aapl_tradexyz_taker', 'equities_taker', 'Taker'),
          buildEquitiesStrategy('aapl_tradexyz_maker', 'equities_maker', 'Maker'),
        ]}
      />,
    );

    expect(screen.getByTestId('equities-arb-signal-table')).toBeInTheDocument();
    expect(screen.getByText('Variant')).toBeInTheDocument();
    expect(screen.getByText('Maker Market')).toBeInTheDocument();
    expect(screen.getByText('Hedge Market')).toBeInTheDocument();

    const strategyCells = screen.getAllByText(/aapl_tradexyz_(maker|taker)/i);
    expect(strategyCells.map((cell) => cell.textContent)).toEqual([
      'aapl_tradexyz_maker',
      'aapl_tradexyz_taker',
    ]);
    expect(screen.getByText('Maker')).toBeInTheDocument();
    expect(screen.getByText('Taker')).toBeInTheDocument();
  });

  it('preserves the prior equities observability and trading-status semantics on the shared surface', () => {
    const pausedMaker = buildEquitiesStrategy('aapl_tradexyz_maker', 'equities_maker', 'Maker');
    pausedMaker.params.bot_on = '0';

    const pendingTaker = buildEquitiesStrategy('aapl_tradexyz_taker', 'equities_taker', 'Taker');
    pendingTaker.equities_arb = {
      ...pendingTaker.equities_arb,
      quote_snapshot: {
        ...pendingTaker.equities_arb?.quote_snapshot,
        hedge_ready: false,
        hedge_disabled_reason: 'outside_rth_blocked',
      },
    };

    render(<EquitiesArbSignalTable rows={[pausedMaker, pendingTaker]} />);

    expect(screen.getByText('Hedge')).toBeInTheDocument();
    expect(screen.getByText('Last Updated')).toBeInTheDocument();
    expect(screen.getAllByText('SMART · DAY').length).toBeGreaterThan(0);
    expect(screen.getAllByText('Feed ok · Quote fresh').length).toBeGreaterThan(0);
    expect(screen.getByText('Paused')).toBeInTheDocument();
    expect(screen.getByText('Pending')).toBeInTheDocument();
    expect(screen.getByText('aapl_tradexyz_maker')).toHaveAttribute(
      'title',
      expect.stringContaining('Assumed hedge fee: 1.00 bps'),
    );
  });
});
