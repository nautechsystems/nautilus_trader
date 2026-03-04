import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { TableFilter, type ColumnFilter } from './TableFilter';
import { spacing } from '@/lib/tokens';

describe('TableFilter (compact mode)', () => {
  const columns: ColumnFilter[] = [
    { key: 'coin', label: 'Coin', type: 'text' },
    { key: 'exchange', label: 'Exchange', type: 'text' },
  ];

  it('uses dense spacing when dense=true', () => {
    const { container } = render(
      <TableFilter columns={columns} onFilterChange={() => {}} dense />
    );

    // Expand filters
    const toggle = screen.getByText('Filters');
    fireEvent.click(toggle);

    const headerContainer = toggle.closest('div');
    expect(headerContainer).not.toBeNull();
    expect(headerContainer).toHaveStyle(`padding: ${spacing.padding.dense}`);

    const grid = container.querySelector('.grid');
    expect(grid).not.toBeNull();
    expect(grid).toHaveStyle(`gap: ${spacing.gap.xs}`);
  });
});
