import { render, screen, within } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import { RiskTable, type RiskSortState } from './RiskTable';
import type { RiskGroup } from '../../types';

const DEFAULT_SORT: RiskSortState = { column: 'gross_mv', direction: 'desc' };

const buildRows = (): RiskGroup[] => [
  {
    risk_key: 'POS',
    label: 'Positive',
    net_qty: 1,
    net_mv: 100,
    long_mv: 100,
    short_mv: 0,
    gross_mv: 100,
    abs_net_mv: 100,
    hedge_ratio: 0,
    sources: ['wallet'],
  },
  {
    risk_key: 'NEG',
    label: 'Negative',
    net_qty: -1,
    net_mv: -50,
    long_mv: 0,
    short_mv: -50,
    gross_mv: 50,
    abs_net_mv: 50,
    hedge_ratio: 0,
    sources: ['bybit'],
  },
];

describe('RiskTable', () => {
  it('renders rows with expected metrics', () => {
    const rows = buildRows();
    const onSortChange = vi.fn();

    render(
      <RiskTable
        rows={rows}
        breakdowns={{
          POS: [{ venue: 'wallet', coin: 'USDT', qty_raw: 100, mv_raw: 100, mark_raw: 1, time_display: 'just now' }],
          NEG: [{ venue: 'bybit', coin: 'PLUME_PERP', qty_raw: -1, mv_raw: -50, mark_raw: 50, time_display: '1m ago' }],
        }}
        search=""
        nonZeroOnly={false}
        sort={DEFAULT_SORT}
        onSortChange={onSortChange}
      />,
    );

    expect(screen.getByText('Positive')).toBeInTheDocument();
    expect(screen.getByText('Negative')).toBeInTheDocument();
    expect(screen.getByText('wallet')).toBeInTheDocument();
    expect(screen.getByText('bybit')).toBeInTheDocument();
  });

  it('uses negative color only for negative net MV', () => {
    const rows = buildRows();
    const onSortChange = vi.fn();

    render(
      <RiskTable
        rows={rows}
        breakdowns={{
          POS: [{ venue: 'wallet', coin: 'USDT', qty_raw: 100, mv_raw: 100, mark_raw: 1, time_display: 'just now' }],
          NEG: [{ venue: 'bybit', coin: 'PLUME_PERP', qty_raw: -1, mv_raw: -50, mark_raw: 50, time_display: '1m ago' }],
        }}
        search=""
        nonZeroOnly={false}
        sort={DEFAULT_SORT}
        onSortChange={onSortChange}
      />,
    );

    const positiveRow = screen.getByText('Positive').closest('tr') as HTMLTableRowElement;
    const positiveNetCell = within(positiveRow).getByText('+ $100.00');

    const negativeCandidates = screen.getAllByText('- $50.00');
    const negativeNetCell = negativeCandidates.find((el) =>
      el.className.includes('text-rose-400'),
    ) as HTMLElement;

    expect(positiveNetCell.className).toContain('text-text-primary');
    expect(positiveNetCell.className).not.toContain('text-rose-400');

    expect(negativeNetCell.className).toContain('text-rose-400');
  });
});
