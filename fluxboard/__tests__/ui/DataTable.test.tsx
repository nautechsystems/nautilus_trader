/**
 * DataTable Component Tests
 */

import { describe, it, expect, vi } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { type ColumnDef } from '@tanstack/react-table';
import { DataTable, type DataTableDebugMetrics } from '@/components/ui/table/DataTable';

interface TestData {
  id: number;
  name: string;
  age: number;
  email: string;
}

const mockData: TestData[] = [
  { id: 1, name: 'Alice', age: 30, email: 'alice@example.com' },
  { id: 2, name: 'Bob', age: 25, email: 'bob@example.com' },
  { id: 3, name: 'Charlie', age: 35, email: 'charlie@example.com' },
];

const columns: ColumnDef<TestData>[] = [
  {
    accessorKey: 'id',
    header: 'ID',
  },
  {
    accessorKey: 'name',
    header: 'Name',
  },
  {
    accessorKey: 'age',
    header: 'Age',
  },
  {
    accessorKey: 'email',
    header: 'Email',
  },
];

function getVisibleNameOrder(): string[] {
  return screen
    .getAllByRole('row')
    .slice(1)
    .map((row) => within(row).getAllByRole('cell')[1]?.textContent ?? '');
}

describe('DataTable', () => {
  it('renders table with data', () => {
    render(<DataTable data={mockData} columns={columns} />);

    expect(screen.getByText('Alice')).toBeInTheDocument();
    expect(screen.getByText('Bob')).toBeInTheDocument();
    expect(screen.getByText('Charlie')).toBeInTheDocument();
  });

  it('renders all column headers', () => {
    render(<DataTable data={mockData} columns={columns} />);

    expect(screen.getByText('ID')).toBeInTheDocument();
    expect(screen.getByText('Name')).toBeInTheDocument();
    expect(screen.getByText('Age')).toBeInTheDocument();
    expect(screen.getByText('Email')).toBeInTheDocument();
  });

  it('uses standardized table header typography classes', () => {
    render(<DataTable data={mockData} columns={columns} />);

    const headerCell = screen.getByText('ID').closest('th');
    expect(headerCell).toHaveClass('text-xs', 'font-semibold', 'uppercase');
  });

  it('renders empty state when no data', () => {
    render(<DataTable data={[]} columns={columns} />);

    expect(screen.getByText('No data')).toBeInTheDocument();
  });

  it('renders custom empty message', () => {
    render(
      <DataTable
        data={[]}
        columns={columns}
        emptyMessage="No records found"
      />
    );

    expect(screen.getByText('No records found')).toBeInTheDocument();
  });

  it('renders loading state', () => {
    render(<DataTable data={mockData} columns={columns} loading />);

    expect(screen.getByText('Loading...')).toBeInTheDocument();
    expect(screen.queryByText('Alice')).not.toBeInTheDocument();
  });

  it('supports sorting when sortable is true', async () => {
    const user = userEvent.setup();

    render(<DataTable data={mockData} columns={columns} sortable />);

    // Find the Age column header
    const ageHeader = screen.getByText('Age').closest('th');
    expect(ageHeader).toBeInTheDocument();

    // Click to sort ascending
    if (ageHeader) {
      await user.click(ageHeader);
    }

    // Verify sort indicator appears
    const sortIndicator = ageHeader?.querySelector('span');
    expect(sortIndicator).toBeInTheDocument();
  });

  it('does not sort when sortable is false', () => {
    render(<DataTable data={mockData} columns={columns} sortable={false} />);

    const ageHeader = screen.getByText('Age').closest('th');
    expect(ageHeader).not.toHaveAttribute('role', 'button');
  });

  it('calls onRowClick when row is clicked', async () => {
    const user = userEvent.setup();
    const onRowClick = vi.fn();

    render(
      <DataTable
        data={mockData}
        columns={columns}
        onRowClick={onRowClick}
      />
    );

    const rows = screen.getAllByRole('button');
    const firstRow = rows[0];
    await user.click(firstRow);

    expect(onRowClick).toHaveBeenCalledTimes(1);
    expect(onRowClick).toHaveBeenCalledWith(mockData[0]);
  });

  it('applies dense mode styling', () => {
    const { container } = render(
      <DataTable data={mockData} columns={columns} dense />
    );

    const rows = container.querySelectorAll('tbody tr');
    const firstRow = rows[0];
    expect(firstRow).toHaveStyle({ height: '28px' });
  });

  it('applies normal mode styling', () => {
    const { container } = render(
      <DataTable data={mockData} columns={columns} dense={false} />
    );

    const rows = container.querySelectorAll('tbody tr');
    const firstRow = rows[0];
    expect(firstRow).toHaveStyle({ height: '32px' });
  });

  it('renders correct number of rows', () => {
    render(<DataTable data={mockData} columns={columns} />);

    const rows = screen.getAllByRole('row');
    // 1 header row + 3 data rows = 4 total
    expect(rows).toHaveLength(4);
  });

  it('applies custom className', () => {
    const { container } = render(
      <DataTable
        data={mockData}
        columns={columns}
        className="custom-class"
      />
    );

    const wrapper = container.querySelector('.custom-class');
    expect(wrapper).toBeInTheDocument();
  });

  it('handles row selection when enabled', () => {
    const onRowSelectionChange = vi.fn();

    render(
      <DataTable
        data={mockData}
        columns={columns}
        enableRowSelection
        rowSelection={{}}
        onRowSelectionChange={onRowSelectionChange}
      />
    );

    // Table should render without errors
    expect(screen.getByText('Alice')).toBeInTheDocument();
  });

  it('shows correct cell content for all columns', () => {
    render(<DataTable data={mockData} columns={columns} />);

    // Check first row
    expect(screen.getByText('1')).toBeInTheDocument();
    expect(screen.getByText('Alice')).toBeInTheDocument();
    expect(screen.getByText('30')).toBeInTheDocument();
    expect(screen.getByText('alice@example.com')).toBeInTheDocument();
  });

  it('maintains table structure with thead and tbody', () => {
    const { container } = render(
      <DataTable data={mockData} columns={columns} />
    );

    const thead = container.querySelector('thead');
    const tbody = container.querySelector('tbody');

    expect(thead).toBeInTheDocument();
    expect(tbody).toBeInTheDocument();
  });

  it('does not call onRowClick when not provided', async () => {
    const user = userEvent.setup();

    render(<DataTable data={mockData} columns={columns} />);

    // Should not have clickable rows
    const rows = screen.getAllByRole('row');
    const dataRow = rows[1]; // First data row (after header)

    expect(dataRow).not.toHaveAttribute('role', 'button');
  });

  it('supports liveDataVersion updates without invalidating the core row model when data identity is stable', () => {
    const liveRow = { id: 10, name: 'Initial', age: 42, email: 'stable@example.com' };
    const liveData = [liveRow];
    const metrics: DataTableDebugMetrics[] = [];

    const { rerender } = render(
      <DataTable
        data={liveData}
        columns={columns}
        getRowId={(row) => String(row.id)}
        liveDataVersion={1}
        onDebugMetrics={(next) => metrics.push(next)}
      />
    );

    expect(screen.getByText('Initial')).toBeInTheDocument();

    liveRow.name = 'Updated';

    rerender(
      <DataTable
        data={liveData}
        columns={columns}
        getRowId={(row) => String(row.id)}
        liveDataVersion={2}
        onDebugMetrics={(next) => metrics.push(next)}
      />
    );

    expect(screen.getByText('Updated')).toBeInTheDocument();
    expect(metrics.at(-1)).toMatchObject({
      coreRowModelInvalidated: false,
      liveCacheReset: true,
    });
  });

  it('recomputes accessor-backed sort order when liveDataVersion changes but data identity stays stable', async () => {
    const user = userEvent.setup();
    const liveData = [
      { id: 1, name: 'Alpha', age: 20, email: 'alpha@example.com' },
      { id: 2, name: 'Bravo', age: 30, email: 'bravo@example.com' },
    ];
    const metrics: DataTableDebugMetrics[] = [];

    const { rerender } = render(
      <DataTable
        data={liveData}
        columns={columns}
        sortable
        getRowId={(row) => String(row.id)}
        liveDataVersion={1}
        onDebugMetrics={(next) => metrics.push(next)}
      />
    );

    await user.click(screen.getByText('Age'));
    expect(getVisibleNameOrder()).toEqual(['Bravo', 'Alpha']);

    liveData[0].age = 40;

    rerender(
      <DataTable
        data={liveData}
        columns={columns}
        sortable
        getRowId={(row) => String(row.id)}
        liveDataVersion={2}
        onDebugMetrics={(next) => metrics.push(next)}
      />
    );

    expect(getVisibleNameOrder()).toEqual(['Alpha', 'Bravo']);
    expect(metrics.at(-1)).toMatchObject({
      coreRowModelInvalidated: true,
      liveCacheReset: true,
    });
  });

  it('keeps the unsorted stable-data fast path when sorting state exists but sorting is disabled', () => {
    const liveRow = { id: 10, name: 'Initial', age: 42, email: 'stable@example.com' };
    const liveData = [liveRow];
    const metrics: DataTableDebugMetrics[] = [];

    const { rerender } = render(
      <DataTable
        data={liveData}
        columns={columns}
        sortable={false}
        initialSorting={[{ id: 'age', desc: true }]}
        getRowId={(row) => String(row.id)}
        liveDataVersion={1}
        onDebugMetrics={(next) => metrics.push(next)}
      />
    );

    liveRow.name = 'Updated';

    rerender(
      <DataTable
        data={liveData}
        columns={columns}
        sortable={false}
        initialSorting={[{ id: 'age', desc: true }]}
        getRowId={(row) => String(row.id)}
        liveDataVersion={2}
        onDebugMetrics={(next) => metrics.push(next)}
      />
    );

    expect(screen.getByText('Updated')).toBeInTheDocument();
    expect(metrics.at(-1)).toMatchObject({
      coreRowModelInvalidated: false,
      liveCacheReset: true,
    });
  });

  it('keeps the unsorted stable-data fast path when sorting state targets a missing column', () => {
    const liveRow = { id: 10, name: 'Initial', age: 42, email: 'stable@example.com' };
    const liveData = [liveRow];
    const metrics: DataTableDebugMetrics[] = [];
    const columnsWithoutAge = columns.filter((column) => (column as { accessorKey?: string }).accessorKey !== 'age');

    const { rerender } = render(
      <DataTable
        data={liveData}
        columns={columnsWithoutAge}
        sortable
        initialSorting={[{ id: 'age', desc: true }]}
        getRowId={(row) => String(row.id)}
        liveDataVersion={1}
        onDebugMetrics={(next) => metrics.push(next)}
      />
    );

    liveRow.name = 'Updated';

    rerender(
      <DataTable
        data={liveData}
        columns={columnsWithoutAge}
        sortable
        initialSorting={[{ id: 'age', desc: true }]}
        getRowId={(row) => String(row.id)}
        liveDataVersion={2}
        onDebugMetrics={(next) => metrics.push(next)}
      />
    );

    expect(screen.getByText('Updated')).toBeInTheDocument();
    expect(metrics.at(-1)).toMatchObject({
      coreRowModelInvalidated: false,
      liveCacheReset: true,
    });
  });

  it('keeps the unsorted stable-data fast path when sorting state targets a non-sortable column', () => {
    const liveRow = { id: 10, name: 'Initial', age: 42, email: 'stable@example.com' };
    const liveData = [liveRow];
    const metrics: DataTableDebugMetrics[] = [];
    const columnsWithStaticAge = columns.map((column) => (
      (column as { accessorKey?: string }).accessorKey === 'age'
        ? { ...column, enableSorting: false }
        : column
    ));

    const { rerender } = render(
      <DataTable
        data={liveData}
        columns={columnsWithStaticAge}
        sortable
        initialSorting={[{ id: 'age', desc: true }]}
        getRowId={(row) => String(row.id)}
        liveDataVersion={1}
        onDebugMetrics={(next) => metrics.push(next)}
      />
    );

    liveRow.name = 'Updated';

    rerender(
      <DataTable
        data={liveData}
        columns={columnsWithStaticAge}
        sortable
        initialSorting={[{ id: 'age', desc: true }]}
        getRowId={(row) => String(row.id)}
        liveDataVersion={2}
        onDebugMetrics={(next) => metrics.push(next)}
      />
    );

    expect(screen.getByText('Updated')).toBeInTheDocument();
    expect(metrics.at(-1)).toMatchObject({
      coreRowModelInvalidated: false,
      liveCacheReset: true,
    });
  });

  it('keeps the unsorted stable-data fast path when sorting state targets an id-only display column', () => {
    const liveRow = { id: 10, name: 'Initial', age: 42, email: 'stable@example.com' };
    const liveData = [liveRow];
    const metrics: DataTableDebugMetrics[] = [];
    const displayOnlyColumns: ColumnDef<TestData>[] = [
      columns[1]!,
      {
        id: 'status',
        header: 'Status',
        cell: ({ row }) => (row.original.age >= 40 ? 'Active' : 'Idle'),
      },
    ];

    const { rerender } = render(
      <DataTable
        data={liveData}
        columns={displayOnlyColumns}
        sortable
        initialSorting={[{ id: 'status', desc: true }]}
        getRowId={(row) => String(row.id)}
        liveDataVersion={1}
        onDebugMetrics={(next) => metrics.push(next)}
      />
    );

    liveRow.name = 'Updated';

    rerender(
      <DataTable
        data={liveData}
        columns={displayOnlyColumns}
        sortable
        initialSorting={[{ id: 'status', desc: true }]}
        getRowId={(row) => String(row.id)}
        liveDataVersion={2}
        onDebugMetrics={(next) => metrics.push(next)}
      />
    );

    expect(screen.getByText('Updated')).toBeInTheDocument();
    expect(metrics.at(-1)).toMatchObject({
      coreRowModelInvalidated: false,
      liveCacheReset: true,
    });
  });

  it('recomputes accessorFn sort order when the runtime sort id comes from the string header', () => {
    const liveData = [
      { id: 1, name: 'Alpha', age: 20, email: 'alpha@example.com' },
      { id: 2, name: 'Bravo', age: 30, email: 'bravo@example.com' },
    ];
    const metrics: DataTableDebugMetrics[] = [];
    const accessorFnColumns: ColumnDef<TestData>[] = [
      columns[1]!,
      {
        header: 'Derived Age',
        accessorFn: (row) => row.age,
      },
    ];
    const getVisibleDerivedAgeNameOrder = () => screen
      .getAllByRole('row')
      .slice(1)
      .map((row) => within(row).getAllByRole('cell')[0]?.textContent ?? '');

    const { rerender } = render(
      <DataTable
        data={liveData}
        columns={accessorFnColumns}
        sortable
        initialSorting={[{ id: 'Derived Age', desc: true }]}
        getRowId={(row) => String(row.id)}
        liveDataVersion={1}
        onDebugMetrics={(next) => metrics.push(next)}
      />
    );

    expect(getVisibleDerivedAgeNameOrder()).toEqual(['Bravo', 'Alpha']);

    liveData[0].age = 40;

    rerender(
      <DataTable
        data={liveData}
        columns={accessorFnColumns}
        sortable
        initialSorting={[{ id: 'Derived Age', desc: true }]}
        getRowId={(row) => String(row.id)}
        liveDataVersion={2}
        onDebugMetrics={(next) => metrics.push(next)}
      />
    );

    expect(getVisibleDerivedAgeNameOrder()).toEqual(['Alpha', 'Bravo']);
    expect(metrics.at(-1)).toMatchObject({
      coreRowModelInvalidated: true,
      liveCacheReset: true,
    });
  });

  it('measures rendered virtual rows for variable-height virtualization', () => {
    const measureElement = vi.fn();
    const virtualizer = {
      getVirtualItems: () => [{ index: 0, start: 0, size: 32 }],
      getTotalSize: () => 32,
      measureElement,
    };

    render(
      <DataTable
        data={[mockData[0]]}
        columns={columns}
        getRowId={(row) => String(row.id)}
        virtualizer={virtualizer as any}
      />
    );

    expect(measureElement).toHaveBeenCalled();
    expect(screen.getByText('Alice').closest('tr')).toHaveAttribute('data-index', '0');
  });
});
