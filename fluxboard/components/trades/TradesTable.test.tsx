import { describe, expect, it, vi } from 'vitest';
import { render } from '@testing-library/react';
import { TradesTable } from './TradesTable';

vi.mock('@tanstack/react-table', () => ({
  Cell: {},
  Row: {},
  flexRender: () => null,
  getCoreRowModel: () => () => ({ rows: [] }),
  useReactTable: () => ({
    getRowModel: () => ({ rows: [] }),
    getHeaderGroups: () => [],
  }),
}));

vi.mock('@tanstack/react-virtual', () => ({
  useVirtualizer: () => ({
    getVirtualItems: () => [],
    getTotalSize: () => 0,
  }),
}));

vi.mock('./columns', () => ({
  createColumns: () => [],
}));

vi.mock('./DecisionModal', () => ({
  DecisionModal: () => null,
}));

describe('TradesTable enableDecisionDetails prop', () => {
  it('renders when decision details are enabled', () => {
    expect(() => {
      render(<TradesTable trades={[]} enableDecisionDetails />);
    }).not.toThrow();
  });
});
