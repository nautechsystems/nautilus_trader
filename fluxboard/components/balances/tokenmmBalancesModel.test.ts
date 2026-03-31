import { describe, expect, it } from 'vitest';

import type { BalancesPayload, BalanceParentRow } from '../../types';
import {
  buildTokenMMBalanceSearchTokens,
  buildTokenMMBalancesViewModel,
  classifyTokenMMBalanceRowType,
  createDefaultTokenMMBalancesToolbarState,
  deriveTokenMMVenueOptions,
  getTokenMMBalanceReadableLabel,
  groupTokenMMBalanceSections,
  reconcileExpandedTokenMMBalanceIds,
} from './tokenmmBalancesModel';

function buildPayload(): BalancesPayload {
  const generatedAt = Date.parse('2026-03-31T00:10:00.000Z');

  const rows: BalanceParentRow[] = [
    {
      id: 'USDC_LOGICAL',
      coin: 'USDC_LOGICAL',
      canonical: 'USDC',
      is_parent: true,
      stable: true,
      qty_display: '1200',
      qty_raw: 1200,
      mv_display: '$1200.00',
      mv_raw: 1200,
      mark_display: '1.00',
      mark_raw: 1,
      time_display: new Date(generatedAt - 5_000).toISOString(),
      time_iso: new Date(generatedAt - 5_000).toISOString(),
      last_ts: generatedAt - 5_000,
      raw: {
        qty: 1200,
        mv_usd: 1200,
        mark: 1,
      },
      children: [
        {
          id: 'USDC_LOGICAL:bybit:spot',
          parent_id: 'USDC_LOGICAL',
          coin: 'USDC',
          venue: 'bybit',
          wallet: 'bybit-main',
          display_name_short: 'Bybit USDC Spot',
          product_type: 'spot',
          contract_type: 'cash',
          raw_symbol: 'USDC',
          qty_display: '1200',
          qty_raw: 1200,
          mv_display: '$1200.00',
          mv_raw: 1200,
          mark_display: '1.00',
          mark_raw: 1,
          time_display: new Date(generatedAt - 5_000).toISOString(),
          time_iso: new Date(generatedAt - 5_000).toISOString(),
          last_ts: generatedAt - 5_000,
        },
      ],
    },
    {
      id: 'BTC_LOGICAL',
      coin: 'BTC_LOGICAL',
      canonical: 'BTC',
      is_parent: true,
      stable: false,
      qty_display: '2.25',
      qty_raw: 2.25,
      mv_display: '$125.00',
      mv_raw: 125,
      mark_display: '55.56',
      mark_raw: 55.56,
      time_display: new Date(generatedAt - 120_000).toISOString(),
      time_iso: new Date(generatedAt - 120_000).toISOString(),
      last_ts: generatedAt - 120_000,
      raw: {
        qty: 2.25,
        mv_usd: 125,
        mark: 55.56,
      },
      children: [
        {
          id: 'BTC_LOGICAL:bybit:spot',
          parent_id: 'BTC_LOGICAL',
          coin: 'BTC',
          venue: 'bybit',
          wallet: 'bybit-main',
          display_name_short: 'Bybit BTC Spot',
          product_type: 'spot',
          contract_type: 'cash',
          raw_symbol: 'BTC',
          qty_display: '1',
          qty_raw: 1,
          mv_display: '$50.00',
          mv_raw: 50,
          mark_display: '50.00',
          mark_raw: 50,
          time_display: new Date(generatedAt - 120_000).toISOString(),
          time_iso: new Date(generatedAt - 120_000).toISOString(),
          last_ts: generatedAt - 120_000,
        },
        {
          id: 'BTC_LOGICAL:okx:perp',
          parent_id: 'BTC_LOGICAL',
          coin: 'BTC',
          venue: 'okx',
          wallet: 'okx-main',
          display_name_short: 'OKX BTC Perp',
          product_type: 'perp',
          contract_type: 'linear',
          raw_symbol: 'BTC-USDT-SWAP',
          instrument_id: 'BTC-USDT-SWAP.OKX',
          qty_display: '1',
          qty_raw: 1,
          mv_display: '$50.00',
          mv_raw: 50,
          mark_display: '50.00',
          mark_raw: 50,
          time_display: new Date(generatedAt - 120_000).toISOString(),
          time_iso: new Date(generatedAt - 120_000).toISOString(),
          last_ts: generatedAt - 120_000,
        },
        {
          id: 'BTC_LOGICAL:wallet:cash',
          parent_id: 'BTC_LOGICAL',
          coin: 'BTC',
          venue: 'wallet',
          wallet: 'treasury',
          contract_type: 'cash',
          raw_symbol: 'WBTC',
          qty_display: '0.25',
          qty_raw: 0.25,
          mv_display: '$25.00',
          mv_raw: 25,
          mark_display: '100.00',
          mark_raw: 100,
          time_display: new Date(generatedAt - 120_000).toISOString(),
          time_iso: new Date(generatedAt - 120_000).toISOString(),
          last_ts: generatedAt - 120_000,
        },
      ],
    },
    {
      id: 'PLUME_LOGICAL',
      coin: 'PLUME_LOGICAL',
      canonical: 'PLUME',
      is_parent: true,
      stable: false,
      qty_display: '1500',
      qty_raw: 1500,
      mv_display: '$75.00',
      mv_raw: 75,
      mark_display: '0.05',
      mark_raw: 0.05,
      time_display: new Date(generatedAt - 5_000).toISOString(),
      time_iso: new Date(generatedAt - 5_000).toISOString(),
      last_ts: generatedAt - 5_000,
      raw: {
        qty: 1500,
        mv_usd: 75,
        mark: 0.05,
      },
      children: [
        {
          id: 'PLUME_LOGICAL:bybit:perp',
          parent_id: 'PLUME_LOGICAL',
          coin: 'PLUME',
          venue: 'bybit',
          wallet: 'bybit-main',
          display_name_short: 'Bybit PLUME Perp',
          product_type: 'perp',
          contract_type: 'linear',
          instrument_id: 'PLUMEUSDT-LINEAR.BYBIT',
          raw_symbol: 'PLUMEUSDT',
          qty_display: '1000',
          qty_raw: 1000,
          mv_display: '$50.00',
          mv_raw: 50,
          mark_display: '0.05',
          mark_raw: 0.05,
          time_display: new Date(generatedAt - 120_000).toISOString(),
          time_iso: new Date(generatedAt - 120_000).toISOString(),
          last_ts: generatedAt - 120_000,
        },
        {
          id: 'PLUME_LOGICAL:wallet:cash',
          parent_id: 'PLUME_LOGICAL',
          coin: 'PLUME',
          venue: 'wallet',
          wallet: 'treasury',
          contract_type: 'cash',
          raw_symbol: 'WPLUME',
          qty_display: '500',
          qty_raw: 500,
          mv_display: '$25.00',
          mv_raw: 25,
          mark_display: '0.05',
          mark_raw: 0.05,
          time_display: new Date(generatedAt - 5_000).toISOString(),
          time_iso: new Date(generatedAt - 5_000).toISOString(),
          last_ts: generatedAt - 5_000,
        },
      ],
    },
    {
      id: 'ZERO_LOGICAL',
      coin: 'ZERO_LOGICAL',
      canonical: 'ZERO',
      is_parent: true,
      stable: false,
      qty_display: '0',
      qty_raw: 0,
      mv_display: '$0.00',
      mv_raw: 0,
      mark_display: '0.00',
      mark_raw: 0,
      time_display: new Date(generatedAt - 5_000).toISOString(),
      time_iso: new Date(generatedAt - 5_000).toISOString(),
      last_ts: generatedAt - 5_000,
      raw: {
        qty: 0,
        mv_usd: 0,
        mark: 0,
      },
      children: [
        {
          id: 'ZERO_LOGICAL:wallet:cash',
          parent_id: 'ZERO_LOGICAL',
          coin: 'ZERO',
          venue: 'wallet',
          wallet: 'dust-wallet',
          contract_type: 'cash',
          raw_symbol: 'ZERO',
          qty_display: '0',
          qty_raw: 0,
          mv_display: '$0.00',
          mv_raw: 0,
          mark_display: '0.00',
          mark_raw: 0,
          time_display: new Date(generatedAt - 5_000).toISOString(),
          time_iso: new Date(generatedAt - 5_000).toISOString(),
          last_ts: generatedAt - 5_000,
        },
      ],
    },
  ];

  return {
    rows,
    total: rows.length,
    totals: {
      mv_raw: 1400,
      mv_display: '$1400.00',
    },
    generated_at: new Date(generatedAt).toISOString(),
    view: 'parents_only',
    stale_after_ms: 30_000,
    degraded: false,
    scope_status: [],
  };
}

describe('tokenmmBalancesModel', () => {
  it('classifies child rows into spot, perp, and cash', () => {
    const rows = buildPayload().rows;
    expect(classifyTokenMMBalanceRowType(rows[0]!.children[0]!)).toBe('spot');
    expect(classifyTokenMMBalanceRowType(rows[1]!.children[1]!)).toBe('perp');
    expect(classifyTokenMMBalanceRowType(rows[1]!.children[2]!)).toBe('cash');
  });

  it('groups parent rows into stable and trading sections', () => {
    const rows = buildPayload().rows;
    expect(groupTokenMMBalanceSections(rows)).toEqual({
      stables: [rows[0]],
      trading: [rows[1], rows[2], rows[3]],
    });
  });

  it('derives sorted venue options from current child rows', () => {
    const rows = buildPayload().rows;
    expect(deriveTokenMMVenueOptions(rows)).toEqual([
      { label: 'All venues', value: 'all' },
      { label: 'Bybit', value: 'bybit' },
      { label: 'Okx', value: 'okx' },
      { label: 'Wallet', value: 'wallet' },
    ]);
  });

  it('prefers backend display names and falls back to normalized instrument labels', () => {
    const rows = buildPayload().rows;
    expect(getTokenMMBalanceReadableLabel(rows[2]!.children[0]!)).toBe('Bybit PLUME Perp');
    expect(getTokenMMBalanceReadableLabel(rows[2]!.children[1]!)).toBe('WPLUME');
  });

  it('builds search tokens from coin, venue, labels, and account text', () => {
    const rows = buildPayload().rows;
    expect(buildTokenMMBalanceSearchTokens(rows[2]!.children[0]!)).toContain('bybit plume perp bybit-main plumeusdt');
  });

  it('builds the tokenmm balances view model with status, sorting, summaries, and filters', () => {
    const payload = buildPayload();
    const viewModel = buildTokenMMBalancesViewModel(
      payload,
      createDefaultTokenMMBalancesToolbarState(),
      ['BTC_LOGICAL', 'MISSING_LOGICAL'],
    );

    expect(viewModel.sections.stables.map((row) => row.id)).toEqual(['USDC_LOGICAL']);
    expect(viewModel.sections.trading.map((row) => row.id)).toEqual(['BTC_LOGICAL', 'PLUME_LOGICAL']);
    expect(viewModel.sections.trading[0]!.status).toBe('STALE');
    expect(viewModel.sections.trading[1]!.status).toBe('PARTIAL');
    expect(viewModel.sections.trading[0]!.children.map((row) => row.id)).toEqual([
      'BTC_LOGICAL:bybit:spot',
      'BTC_LOGICAL:okx:perp',
      'BTC_LOGICAL:wallet:cash',
    ]);
    expect(viewModel.summary).toMatchObject({
      totalMv: 1400,
      stableMv: 1200,
      nonStableMv: 200,
      nonZeroCoinCount: 3,
      staleRowCount: 2,
    });
    expect(viewModel.expandedParentIds).toEqual(new Set(['BTC_LOGICAL']));
    expect(viewModel.venueOptions).toEqual([
      { label: 'All venues', value: 'all' },
      { label: 'Bybit', value: 'bybit' },
      { label: 'Okx', value: 'okx' },
      { label: 'Wallet', value: 'wallet' },
    ]);

    const okxFilter = buildTokenMMBalancesViewModel(
      payload,
      { ...createDefaultTokenMMBalancesToolbarState(), venue: 'okx' },
    );
    expect(okxFilter.sections.trading.map((row) => row.id)).toEqual(['BTC_LOGICAL']);

    const perpFilter = buildTokenMMBalancesViewModel(
      payload,
      { ...createDefaultTokenMMBalancesToolbarState(), type: 'perp' },
    );
    expect(perpFilter.sections.trading.map((row) => row.id)).toEqual(['BTC_LOGICAL', 'PLUME_LOGICAL']);

    const searchFilter = buildTokenMMBalancesViewModel(
      payload,
      { ...createDefaultTokenMMBalancesToolbarState(), search: 'wallet' },
    );
    expect(searchFilter.sections.trading.map((row) => row.id)).toEqual(['BTC_LOGICAL', 'PLUME_LOGICAL']);
  });

  it('reconciles expanded ids against the latest parent rows', () => {
    expect(reconcileExpandedTokenMMBalanceIds(['BTC_LOGICAL', 'GONE_LOGICAL'], buildPayload().rows)).toEqual(
      new Set(['BTC_LOGICAL']),
    );
  });
});
