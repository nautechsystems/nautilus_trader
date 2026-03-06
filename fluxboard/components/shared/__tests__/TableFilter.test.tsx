// TableFilter component unit tests

import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import { TableFilter, applyFilters, type FilterValues, type ColumnFilter } from '../TableFilter';
import { spacing } from '@/lib/tokens';

describe('TableFilter Component', () => {
  const mockColumns: ColumnFilter[] = [
    { key: 'name', label: 'Name', type: 'text', placeholder: 'Search name...' },
    { key: 'status', label: 'Status', type: 'select', options: ['active', 'inactive'] },
    { key: 'category', label: 'Category', type: 'text', placeholder: 'Category...' }
  ];

  const mockOnFilterChange = vi.fn();

  it('renders with collapsed state by default', () => {
    render(<TableFilter columns={mockColumns} onFilterChange={mockOnFilterChange} />);

    // Filter button should be visible
    expect(screen.getByText('Filters')).toBeInTheDocument();

    // Filter inputs should not be visible initially
    expect(screen.queryByPlaceholderText('Search name...')).not.toBeInTheDocument();
  });

  it('expands to show filter inputs when clicked', () => {
    render(<TableFilter columns={mockColumns} onFilterChange={mockOnFilterChange} />);

    const filterButton = screen.getByText('Filters');
    fireEvent.click(filterButton);

    // Filter inputs should now be visible
    expect(screen.getByPlaceholderText('Search name...')).toBeInTheDocument();
    expect(screen.getByPlaceholderText('Category...')).toBeInTheDocument();
  });

  it('shows active filter count badge', () => {
    render(<TableFilter columns={mockColumns} onFilterChange={mockOnFilterChange} />);

    const filterButton = screen.getByText('Filters');
    fireEvent.click(filterButton);

    const nameInput = screen.getByPlaceholderText('Search name...');
    fireEvent.change(nameInput, { target: { value: 'test' } });

    // Badge should show "1"
    expect(screen.getByText('1')).toBeInTheDocument();
  });

  it('calls onFilterChange when filter value changes', () => {
    render(<TableFilter columns={mockColumns} onFilterChange={mockOnFilterChange} />);

    const filterButton = screen.getByText('Filters');
    fireEvent.click(filterButton);

    const nameInput = screen.getByPlaceholderText('Search name...');
    fireEvent.change(nameInput, { target: { value: 'test' } });

    expect(mockOnFilterChange).toHaveBeenCalledWith({ name: 'test' });
  });

  it('renders select filter with options', () => {
    render(<TableFilter columns={mockColumns} onFilterChange={mockOnFilterChange} />);

    const filterButton = screen.getByText('Filters');
    fireEvent.click(filterButton);

    const statusSelect = screen.getByDisplayValue('All');
    expect(statusSelect).toBeInTheDocument();

    // Check options
    const options = (statusSelect as HTMLSelectElement).options;
    expect(options).toHaveLength(3); // Empty + 2 options
    expect(options[1].value).toBe('active');
    expect(options[2].value).toBe('inactive');
  });

  it('clears all filters when Clear button clicked', () => {
    render(<TableFilter columns={mockColumns} onFilterChange={mockOnFilterChange} />);

    const filterButton = screen.getByText('Filters');
    fireEvent.click(filterButton);

    // Apply some filters
    const nameInput = screen.getByPlaceholderText('Search name...');
    fireEvent.change(nameInput, { target: { value: 'test' } });

    const categoryInput = screen.getByPlaceholderText('Category...');
    fireEvent.change(categoryInput, { target: { value: 'work' } });

    // Clear filters
    const clearButton = screen.getByText('Clear All');
    fireEvent.click(clearButton);

    // onFilterChange should be called with empty object
    expect(mockOnFilterChange).toHaveBeenCalledWith({});
    expect(nameInput).toHaveValue('');
    expect(categoryInput).toHaveValue('');
  });

  it('renders in dense mode with smaller padding', () => {
    render(
      <TableFilter columns={mockColumns} onFilterChange={mockOnFilterChange} dense={true} />
    );

    const filterButton = screen.getByText('Filters');
    const header = filterButton.closest('div');
    expect(header).not.toBeNull();
    expect(header).toHaveStyle(`padding: ${spacing.padding.dense}`);
  });

  it('maintains filter state when toggling expand/collapse', () => {
    render(<TableFilter columns={mockColumns} onFilterChange={mockOnFilterChange} />);

    const filterButton = screen.getByText('Filters');

    // Expand
    fireEvent.click(filterButton);

    const nameInput = screen.getByPlaceholderText('Search name...');
    fireEvent.change(nameInput, { target: { value: 'test' } });

    // Collapse
    fireEvent.click(filterButton);

    // Expand again
    fireEvent.click(filterButton);

    // Filter value should be preserved
    expect(nameInput).toHaveValue('test');
  });
});

describe('applyFilters Utility', () => {
  const mockColumns: ColumnFilter[] = [
    { key: 'name', label: 'Name', type: 'text' },
    { key: 'status', label: 'Status', type: 'select', options: ['active', 'inactive'] },
    { key: 'category', label: 'Category', type: 'text' },
  ];

  const mockRows = [
    { id: 1, name: 'Alice', status: 'active', category: 'work' },
    { id: 2, name: 'Bob', status: 'inactive', category: 'personal' },
    { id: 3, name: 'Charlie', status: 'active', category: 'work' },
    { id: 4, name: 'David', status: 'active', category: 'hobby' }
  ];

  it('returns all rows when no filters applied', () => {
    const filters: FilterValues = {};
    const result = applyFilters(mockRows, filters, { columns: mockColumns });

    expect(result).toEqual(mockRows);
    expect(result).toHaveLength(4);
  });

  it('filters by single text field (case-insensitive)', () => {
    const filters: FilterValues = { name: 'ali' };
    const result = applyFilters(mockRows, filters, { columns: mockColumns });

    expect(result).toHaveLength(1);
    expect(result[0].name).toBe('Alice');
  });

  it('filters select fields using case-insensitive equality', () => {
    const filters: FilterValues = { status: 'active' };
    const result = applyFilters(mockRows, filters, { columns: mockColumns });

    expect(result).toHaveLength(3);
    expect(result.every(row => row.status === 'active')).toBe(true);
  });

  it('filters select fields against tokenized multi-value strings', () => {
    const rows = [
      { id: 1, market_type: 'perp spot' },
      { id: 2, market_type: 'spot' },
      { id: 3, market_type: 'perp' },
    ];
    const columns: ColumnFilter[] = [
      { key: 'market_type', label: 'Market', type: 'select', options: ['spot', 'perp'] },
    ];

    const result = applyFilters(rows, { market_type: 'perp' }, { columns });

    expect(result).toEqual([
      { id: 1, market_type: 'perp spot' },
      { id: 3, market_type: 'perp' },
    ]);
  });

  it('filters by multiple fields (AND logic)', () => {
    const filters: FilterValues = { status: 'active', category: 'work' };
    const result = applyFilters(mockRows, filters, { columns: mockColumns });

    expect(result).toHaveLength(2);
    expect(result[0].name).toBe('Alice');
    expect(result[1].name).toBe('Charlie');
  });

  it('returns empty array when no rows match', () => {
    const filters: FilterValues = { name: 'nonexistent' };
    const result = applyFilters(mockRows, filters, { columns: mockColumns });

    expect(result).toHaveLength(0);
  });

  it('handles partial matches for text fields', () => {
    const filters: FilterValues = { category: 'ork' };
    const result = applyFilters(mockRows, filters, { columns: mockColumns });

    expect(result).toHaveLength(2);
    expect(result.every(row => row.category === 'work')).toBe(true);
  });

  it('handles empty string filters (shows all)', () => {
    const filters: FilterValues = { name: '', status: '' };
    const result = applyFilters(mockRows, filters, { columns: mockColumns });

    expect(result).toEqual(mockRows);
  });

  it('handles missing fields in rows gracefully', () => {
    const rowsWithMissing = [
      { id: 1, name: 'Alice' },
      { id: 2, name: 'Bob', status: 'active' }
    ];

    const filters: FilterValues = { status: 'active' };
    const result = applyFilters(rowsWithMissing as any, filters, { columns: mockColumns });

    expect(result).toHaveLength(1);
    expect(result[0].name).toBe('Bob');
  });

  it('supports custom matcher functions for specialized fields', () => {
    const rows = [
      { id: 1, coin: 'WETH/USDT' },
      { id: 2, coin: 'BTC/USDT' },
      { id: 3, coin: 'WPLUME/PUSD' },
    ];
    const filters: FilterValues = { coin: 'eth' };
    const result = applyFilters(rows, filters, {
      matchers: {
        coin: (row, filterValue) => {
          const search = filterValue.toLowerCase();
          const coin = String(row.coin || '').toLowerCase();
          const base = coin.split('/')[0] || '';
          const unwrapped = base.startsWith('w') ? base.slice(1) : base;
          return coin.includes(search) || base.includes(search) || unwrapped.includes(search);
        },
      },
    });

    expect(result).toHaveLength(1);
    expect(result[0].coin).toBe('WETH/USDT');
  });
});
