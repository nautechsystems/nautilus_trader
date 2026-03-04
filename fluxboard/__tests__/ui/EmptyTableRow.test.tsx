/**
 * EmptyTableRow Component Tests
 */

import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { EmptyTableRow } from '@/components/ui/table/EmptyTableRow';

describe('EmptyTableRow', () => {
  it('renders default message', () => {
    render(
      <table>
        <tbody>
          <EmptyTableRow colSpan={3} />
        </tbody>
      </table>
    );

    expect(screen.getByText('No data')).toBeInTheDocument();
  });

  it('renders custom message', () => {
    render(
      <table>
        <tbody>
          <EmptyTableRow colSpan={3} message="No trades found" />
        </tbody>
      </table>
    );

    expect(screen.getByText('No trades found')).toBeInTheDocument();
  });

  it('spans correct number of columns', () => {
    const { container } = render(
      <table>
        <tbody>
          <EmptyTableRow colSpan={5} />
        </tbody>
      </table>
    );

    const cell = container.querySelector('td');
    expect(cell).toHaveAttribute('colSpan', '5');
  });

  it('renders icon when provided', () => {
    const icon = <svg data-testid="search-icon">Icon</svg>;

    render(
      <table>
        <tbody>
          <EmptyTableRow colSpan={3} icon={icon} />
        </tbody>
      </table>
    );

    expect(screen.getByTestId('search-icon')).toBeInTheDocument();
  });

  it('renders without icon when not provided', () => {
    render(
      <table>
        <tbody>
          <EmptyTableRow colSpan={3} />
        </tbody>
      </table>
    );

    const cell = screen.getByText('No data').closest('td');
    const iconContainer = cell?.querySelector('.opacity-50');
    expect(iconContainer).not.toBeInTheDocument();
  });

  it('applies custom className', () => {
    const { container } = render(
      <table>
        <tbody>
          <EmptyTableRow colSpan={3} className="custom-class" />
        </tbody>
      </table>
    );

    const cell = container.querySelector('.custom-class');
    expect(cell).toBeInTheDocument();
  });

  it('has centered text styling', () => {
    const { container } = render(
      <table>
        <tbody>
          <EmptyTableRow colSpan={3} />
        </tbody>
      </table>
    );

    const cell = container.querySelector('td');
    expect(cell).toHaveClass('text-center');
  });

  it('has proper padding', () => {
    const { container } = render(
      <table>
        <tbody>
          <EmptyTableRow colSpan={3} />
        </tbody>
      </table>
    );

    const cell = container.querySelector('td');
    expect(cell).toHaveClass('py-8');
  });
});
