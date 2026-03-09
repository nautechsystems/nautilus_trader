/**
 * TableHeader Component Tests
 */

import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { TableHeader } from '@/components/ui/table/TableHeader';

describe('TableHeader', () => {
  it('renders children content', () => {
    render(
      <table>
        <thead>
          <tr>
            <TableHeader>Column Name</TableHeader>
          </tr>
        </thead>
      </table>
    );

    expect(screen.getByText('Column Name')).toBeInTheDocument();
  });

  it('is not clickable when sortable is false', () => {
    const onSort = vi.fn();
    render(
      <table>
        <thead>
          <tr>
            <TableHeader sortable={false} onSort={onSort}>
              Column
            </TableHeader>
          </tr>
        </thead>
      </table>
    );

    const header = screen.getByRole('columnheader');
    expect(header).not.toHaveAttribute('role', 'button');
    expect(header).not.toHaveAttribute('tabIndex');
  });

  it('is clickable when sortable is true', async () => {
    const user = userEvent.setup();
    const onSort = vi.fn();

    render(
      <table>
        <thead>
          <tr>
            <TableHeader sortable onSort={onSort}>
              Column
            </TableHeader>
          </tr>
        </thead>
      </table>
    );

    const header = screen.getByRole('button');
    expect(header).toHaveAttribute('tabIndex', '0');

    await user.click(header);
    expect(onSort).toHaveBeenCalledTimes(1);
  });

  it('handles keyboard activation with Enter key', async () => {
    const user = userEvent.setup();
    const onSort = vi.fn();

    render(
      <table>
        <thead>
          <tr>
            <TableHeader sortable onSort={onSort}>
              Column
            </TableHeader>
          </tr>
        </thead>
      </table>
    );

    const header = screen.getByRole('button');
    header.focus();
    await user.keyboard('{Enter}');

    expect(onSort).toHaveBeenCalledTimes(1);
  });

  it('handles keyboard activation with Space key', async () => {
    const user = userEvent.setup();
    const onSort = vi.fn();

    render(
      <table>
        <thead>
          <tr>
            <TableHeader sortable onSort={onSort}>
              Column
            </TableHeader>
          </tr>
        </thead>
      </table>
    );

    const header = screen.getByRole('button');
    header.focus();
    await user.keyboard(' ');

    expect(onSort).toHaveBeenCalledTimes(1);
  });

  it('has sticky positioning styles', () => {
    render(
      <table>
        <thead>
          <tr>
            <TableHeader>Column</TableHeader>
          </tr>
        </thead>
      </table>
    );

    const header = screen.getByRole('columnheader');
    expect(header).toHaveClass('sticky', 'top-0');
  });

  it('applies custom className', () => {
    render(
      <table>
        <thead>
          <tr>
            <TableHeader className="custom-class">Column</TableHeader>
          </tr>
        </thead>
      </table>
    );

    const header = screen.getByRole('columnheader');
    expect(header).toHaveClass('custom-class');
  });

  it('does not call onSort when not sortable', async () => {
    const user = userEvent.setup();
    const onSort = vi.fn();

    render(
      <table>
        <thead>
          <tr>
            <TableHeader sortable={false} onSort={onSort}>
              Column
            </TableHeader>
          </tr>
        </thead>
      </table>
    );

    const header = screen.getByRole('columnheader');
    await user.click(header);

    expect(onSort).not.toHaveBeenCalled();
  });
});
