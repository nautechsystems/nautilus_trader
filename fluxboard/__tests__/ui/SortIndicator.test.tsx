/**
 * SortIndicator Component Tests
 */

import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { SortIndicator } from '@/components/ui/table/SortIndicator';

describe('SortIndicator', () => {
  it('renders neutral state when column is not sorted', () => {
    render(
      <SortIndicator
        column="name"
        sortColumn="age"
        sortDirection="asc"
      />
    );

    const indicator = screen.getByText('↕');
    expect(indicator).toBeInTheDocument();
    expect(indicator).toHaveAttribute('aria-label', 'Not sorted');
  });

  it('renders ascending arrow when column is sorted ascending', () => {
    render(
      <SortIndicator
        column="name"
        sortColumn="name"
        sortDirection="asc"
      />
    );

    const indicator = screen.getByText('↑');
    expect(indicator).toBeInTheDocument();
    expect(indicator).toHaveAttribute('aria-label', 'Sorted ascending');
  });

  it('renders descending arrow when column is sorted descending', () => {
    render(
      <SortIndicator
        column="name"
        sortColumn="name"
        sortDirection="desc"
      />
    );

    const indicator = screen.getByText('↓');
    expect(indicator).toBeInTheDocument();
    expect(indicator).toHaveAttribute('aria-label', 'Sorted descending');
  });

  it('renders neutral when sort column is null', () => {
    render(
      <SortIndicator
        column="name"
        sortColumn={null}
        sortDirection={null}
      />
    );

    expect(screen.getByText('↕')).toBeInTheDocument();
  });

  it('applies custom className', () => {
    const { container } = render(
      <SortIndicator
        column="name"
        sortColumn={null}
        sortDirection={null}
        className="custom-class"
      />
    );

    const indicator = container.querySelector('.custom-class');
    expect(indicator).toBeInTheDocument();
  });

  it('has correct inline styles', () => {
    const { container } = render(
      <SortIndicator
        column="name"
        sortColumn={null}
        sortDirection={null}
      />
    );

    const indicator = container.querySelector('span');
    expect(indicator).toHaveStyle({ fontSize: '10px' });
  });
});
