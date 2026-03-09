import { render, screen } from '@testing-library/react';
import type { ColumnDef } from '@tanstack/react-table';
import { DataTable } from './DataTable';

type Row = { a: string; b: string };

const rows: Row[] = [{ a: 'aaa', b: 'bbb' }];

describe('DataTable width modes', () => {
  test('widthMode="content" uses max-content width table and horizontal overflow wrapper', () => {
    const columns: ColumnDef<Row>[] = [
      { accessorKey: 'a', header: 'A' },
      { accessorKey: 'b', header: 'B' },
    ];

    const { container } = render(
      <DataTable<Row>
        data={rows}
        columns={columns}
        widthMode="content"
      />
    );

    const table = container.querySelector('table');
    expect(table).toBeTruthy();
    expect(table?.className).toContain('w-max');

    const wrapper = table?.closest('div');
    expect(wrapper?.className ?? '').toContain('overflow-x-auto');
    expect(wrapper?.className ?? '').toContain('max-w-full');

    // Sanity: table still has headers/cells.
    expect(screen.getByText('A')).toBeInTheDocument();
    expect(screen.getByText('aaa')).toBeInTheDocument();
  });

  test('columnWidthMode="explicit" only applies widths when provided by column defs', () => {
    const columns: ColumnDef<Row>[] = [
      { accessorKey: 'a', header: 'A' },
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      { accessorKey: 'b', header: 'B', minSize: 123 } as any,
    ];

    const { container } = render(
      <DataTable<Row>
        data={rows}
        columns={columns}
        widthMode="content"
        columnWidthMode="explicit"
      />
    );

    const headers = container.querySelectorAll('th');
    expect(headers.length).toBeGreaterThanOrEqual(2);
    expect(headers[0]?.style.width).toBe('');
    expect(headers[0]?.style.minWidth).toBe('');
    expect(headers[1]?.style.minWidth).toBe('123px');

    const cells = container.querySelectorAll('tbody td');
    expect(cells.length).toBeGreaterThanOrEqual(2);
    expect(cells[0]?.style.width).toBe('');
    expect(cells[0]?.style.minWidth).toBe('');
    expect(cells[1]?.style.minWidth).toBe('123px');
  });
});

