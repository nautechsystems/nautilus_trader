/**
 * TableRow Component Tests
 */

import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { TableRow } from '@/components/ui/table/TableRow';

describe('TableRow', () => {
  it('renders children content', () => {
    render(
      <table>
        <tbody>
          <TableRow>
            <td>Cell content</td>
          </TableRow>
        </tbody>
      </table>
    );

    expect(screen.getByText('Cell content')).toBeInTheDocument();
  });

  it('applies dense mode height', () => {
    const { container } = render(
      <table>
        <tbody>
          <TableRow dense>
            <td>Content</td>
          </TableRow>
        </tbody>
      </table>
    );

    const row = container.querySelector('tr');
    expect(row).toHaveStyle({ height: '28px' });
  });

  it('applies normal mode height', () => {
    const { container } = render(
      <table>
        <tbody>
          <TableRow dense={false}>
            <td>Content</td>
          </TableRow>
        </tbody>
      </table>
    );

    const row = container.querySelector('tr');
    expect(row).toHaveStyle({ height: '32px' });
  });

  it('calls onClick when clicked', async () => {
    const user = userEvent.setup();
    const onClick = vi.fn();

    render(
      <table>
        <tbody>
          <TableRow onClick={onClick}>
            <td>Content</td>
          </TableRow>
        </tbody>
      </table>
    );

    const row = screen.getByRole('button');
    await user.click(row);

    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it('handles keyboard activation with Enter', async () => {
    const user = userEvent.setup();
    const onClick = vi.fn();

    render(
      <table>
        <tbody>
          <TableRow onClick={onClick}>
            <td>Content</td>
          </TableRow>
        </tbody>
      </table>
    );

    const row = screen.getByRole('button');
    row.focus();
    await user.keyboard('{Enter}');

    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it('handles keyboard activation with Space', async () => {
    const user = userEvent.setup();
    const onClick = vi.fn();

    render(
      <table>
        <tbody>
          <TableRow onClick={onClick}>
            <td>Content</td>
          </TableRow>
        </tbody>
      </table>
    );

    const row = screen.getByRole('button');
    row.focus();
    await user.keyboard(' ');

    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it('shows selected state', () => {
    const { container } = render(
      <table>
        <tbody>
          <TableRow selected>
            <td>Content</td>
          </TableRow>
        </tbody>
      </table>
    );

    const row = container.querySelector('tr');
    expect(row).toHaveAttribute('aria-selected', 'true');
    expect(row).toHaveClass('bg-accent/12');
  });

  it('is not clickable without onClick handler', () => {
    render(
      <table>
        <tbody>
          <TableRow>
            <td>Content</td>
          </TableRow>
        </tbody>
      </table>
    );

    const row = screen.getByRole('row');
    expect(row).not.toHaveAttribute('role', 'button');
    expect(row).not.toHaveClass('cursor-pointer');
  });

  it('applies custom className', () => {
    const { container } = render(
      <table>
        <tbody>
          <TableRow className="custom-class">
            <td>Content</td>
          </TableRow>
        </tbody>
      </table>
    );

    const row = container.querySelector('.custom-class');
    expect(row).toBeInTheDocument();
  });

  it('has hover styles when not selected', () => {
    const { container } = render(
      <table>
        <tbody>
          <TableRow>
            <td>Content</td>
          </TableRow>
        </tbody>
      </table>
    );

    const row = container.querySelector('tr');
    expect(row).toHaveClass('hover:bg-bg-hover');
  });

  it('does not have hover styles when selected', () => {
    const { container } = render(
      <table>
        <tbody>
          <TableRow selected>
            <td>Content</td>
          </TableRow>
        </tbody>
      </table>
    );

    const row = container.querySelector('tr');
    expect(row).not.toHaveClass('hover:bg-bg-hover');
  });

  it('does not force inline background color so hover and selected styles stay visible', () => {
    const { container } = render(
      <table>
        <tbody>
          <TableRow>
            <td>Content</td>
          </TableRow>
        </tbody>
      </table>
    );

    const row = container.querySelector('tr');
    expect(row?.style.backgroundColor).toBe('');
  });
});
