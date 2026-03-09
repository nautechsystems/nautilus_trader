import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen } from '@testing-library/react';

import { createColumns } from './columns';

describe('Trades columns precision', () => {
  afterEach(() => cleanup());

  const cols = createColumns(vi.fn());

  const renderCellAndAssert = (
    columnId: string,
    value: string | number,
    expectedText: string,
    rowOriginal: object = {},
  ) => {
    const col: any = cols.find((c: any) => c.id === columnId);
    expect(col).toBeTruthy();

    const node = col.cell({
      getValue: () => value,
      row: { original: rowOriginal },
    } as any);

    render(<>{node}</>);
    const el = screen.getByText(expectedText);
    expect(el).toBeInTheDocument();
    return el;
  };

  it('renders price with tick-level precision (e.g., 0.009001)', () => {
    renderCellAndAssert('px', 0.009001, '0.009001');
  });

  it('renders notional with 3-decimal precision when needed (e.g., 9.001)', () => {
    renderCellAndAssert('notional', 9.001, '9.001');
  });

  it('renders fee with >6 decimals when needed (e.g., 0.0009001)', () => {
    renderCellAndAssert('fee', 0.0009001, '0.0009001', { fee_asset_raw: 'USDT' });
  });

  it('renders full fee precision in tooltip when needed', () => {
    const el = renderCellAndAssert('fee', '0.000900123456', '0.00090012', { fee_asset_raw: 'USDT' });
    expect(el.getAttribute('title')).toBe('0.000900123456 USDT');
  });
});
