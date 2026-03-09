// LoadingState tests

import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { LoadingState } from './LoadingState';
import { colors, typography } from '@/lib/tokens';

describe('LoadingState', () => {
  it('renders with default message', () => {
    render(<LoadingState />);
    expect(screen.getByText('Loading...')).toBeInTheDocument();
  });

  it('renders with custom message', () => {
    render(<LoadingState message="Fetching data..." />);
    expect(screen.getByText('Fetching data...')).toBeInTheDocument();
  });

  it('applies custom className', () => {
    const { container } = render(<LoadingState className="custom-class" />);
    const loadingDiv = container.querySelector('.custom-class');
    expect(loadingDiv).toBeInTheDocument();
  });

  it('centers content with flexbox', () => {
    const { container } = render(<LoadingState />);
    const wrapper = container.firstChild as HTMLElement;

    expect(wrapper.classList.contains('flex')).toBe(true);
    expect(wrapper.classList.contains('items-center')).toBe(true);
    expect(wrapper.classList.contains('justify-center')).toBe(true);
  });

  it('fills available height', () => {
    const { container } = render(<LoadingState />);
    const wrapper = container.firstChild as HTMLElement;

    expect(wrapper.classList.contains('h-full')).toBe(true);
  });

  it('applies small size correctly', () => {
    render(<LoadingState size="sm" />);
    const textElement = screen.getByText('Loading...');
    expect(textElement).toHaveStyle({ fontSize: typography.fontSize.xs });
  });

  it('applies medium size correctly (default)', () => {
    render(<LoadingState size="md" />);
    const textElement = screen.getByText('Loading...');
    expect(textElement).toHaveStyle({ fontSize: typography.fontSize.sm });
  });

  it('applies large size correctly', () => {
    render(<LoadingState size="lg" />);
    const textElement = screen.getByText('Loading...');
    expect(textElement).toHaveStyle({ fontSize: typography.fontSize.base });
  });

  it('has neutral gray text color', () => {
    render(<LoadingState />);
    const textElement = screen.getByText('Loading...');
    expect(textElement).toHaveStyle({ color: colors.text.muted });
  });

  it('supports all size variants', () => {
    const sizes: Array<'sm' | 'md' | 'lg'> = ['sm', 'md', 'lg'];

    sizes.forEach(size => {
      const { unmount } = render(<LoadingState size={size} />);
      expect(screen.getByText('Loading...')).toBeInTheDocument();
      unmount();
    });
  });
});
