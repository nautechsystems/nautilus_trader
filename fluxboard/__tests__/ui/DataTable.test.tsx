/**
 * DataTable Component Tests
 */

import { describe, it, expect, vi } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { type ColumnDef } from '@tanstack/react-table';
import { DataTable } from '@/components/ui/table/DataTable';

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
});
