import { describe, expect, it } from 'vitest';

import type { BalanceParentRow } from '../../types';
import {
  buildTokenMMBalanceSearchTokens,
  classifyTokenMMBalanceRowType,
  deriveTokenMMVenueOptions,
  getTokenMMBalanceReadableLabel,
  groupTokenMMBalanceSections,
} from './tokenmmBalancesModel';

function buildRows(): BalanceParentRow[] {
  return [
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
      time_display: '2026-03-31T00:00:00.000Z',
      time_iso: '2026-03-31T00:00:00.000Z',
      last_ts: 1774915200000,
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
          time_display: '2026-03-31T00:00:00.000Z',
          time_iso: '2026-03-31T00:00:00.000Z',
          last_ts: 1774915200000,
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
      time_display: '2026-03-31T00:00:00.000Z',
      time_iso: '2026-03-31T00:00:00.000Z',
      last_ts: 1774915200000,
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
          time_display: '2026-03-31T00:00:00.000Z',
          time_iso: '2026-03-31T00:00:00.000Z',
          last_ts: 1774915200000,
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
          time_display: '2026-03-31T00:00:00.000Z',
          time_iso: '2026-03-31T00:00:00.000Z',
          last_ts: 1774915200000,
        },
      ],
    },
  ];
}

describe('tokenmmBalancesModel', () => {
  it('classifies child rows into spot, perp, and cash', () => {
    const rows = buildRows();
    expect(classifyTokenMMBalanceRowType(rows[0]!.children[0]!)).toBe('spot');
    expect(classifyTokenMMBalanceRowType(rows[1]!.children[0]!)).toBe('perp');
    expect(classifyTokenMMBalanceRowType(rows[1]!.children[1]!)).toBe('cash');
  });

  it('groups parent rows into stable and trading sections', () => {
    const rows = buildRows();
    expect(groupTokenMMBalanceSections(rows)).toEqual({
      stables: [rows[0]],
      trading: [rows[1]],
    });
  });

  it('derives sorted venue options from current child rows', () => {
    const rows = buildRows();
    expect(deriveTokenMMVenueOptions(rows)).toEqual([
      { label: 'All venues', value: 'all' },
      { label: 'Bybit', value: 'bybit' },
      { label: 'Wallet', value: 'wallet' },
    ]);
  });

  it('prefers backend display names and falls back to normalized instrument labels', () => {
    const rows = buildRows();
    expect(getTokenMMBalanceReadableLabel(rows[1]!.children[0]!)).toBe('Bybit PLUME Perp');
    expect(getTokenMMBalanceReadableLabel(rows[1]!.children[1]!)).toBe('WPLUME');
  });

  it('builds search tokens from coin, venue, labels, and account text', () => {
    const rows = buildRows();
    expect(buildTokenMMBalanceSearchTokens(rows[1]!.children[0]!)).toContain('bybit plume perp bybit-main plumeusdt');
  });
});
