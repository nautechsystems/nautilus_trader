// EmptyState tests

import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { EmptyState } from './EmptyState';
import { colors, spacing } from '@/lib/tokens';

describe('EmptyState', () => {
  it('renders with default message', () => {
    render(<EmptyState />);
    expect(screen.getByText('No data found')).toBeInTheDocument();
  });

  it('renders with custom message', () => {
    render(<EmptyState message="No trades available" />);
    expect(screen.getByText('No trades available')).toBeInTheDocument();
  });

  it('renders with icon when provided', () => {
    render(<EmptyState icon="📊" />);
    expect(screen.getByText('📊')).toBeInTheDocument();
  });

  it('does not render icon when not provided', () => {
    const { container } = render(<EmptyState />);
    const iconElement = container.querySelector('.text-4xl');
    expect(iconElement).toBeNull();
  });

  it('applies custom className', () => {
    const { container } = render(<EmptyState className="custom-empty" />);
    const emptyDiv = container.querySelector('.custom-empty');
    expect(emptyDiv).toBeInTheDocument();
  });

  it('centers content vertically and horizontally', () => {
    const { container } = render(<EmptyState />);
    const wrapper = container.firstChild as HTMLElement;

    expect(wrapper.classList.contains('flex')).toBe(true);
    expect(wrapper.classList.contains('flex-col')).toBe(true);
    expect(wrapper.classList.contains('items-center')).toBe(true);
    expect(wrapper.classList.contains('justify-center')).toBe(true);
  });

  it('fills available height', () => {
    const { container } = render(<EmptyState />);
    const wrapper = container.firstChild as HTMLElement;

    expect(wrapper.classList.contains('h-full')).toBe(true);
  });

  it('uses neutral gray text color', () => {
    const { container } = render(<EmptyState />);
    const textElement = container.querySelector('.text-sm') as HTMLElement | null;
    expect(textElement).not.toBeNull();
    expect(textElement).toHaveStyle({ color: colors.text.muted });
  });

  it('uses small text size', () => {
    const { container } = render(<EmptyState />);
    const textElement = container.querySelector('.text-sm');
    expect(textElement).toBeInTheDocument();
  });

  it('renders icon with larger size', () => {
    const { container } = render(<EmptyState icon="🔍" />);
    const iconElement = container.querySelector('.text-4xl');
    expect(iconElement).toBeInTheDocument();
    expect(iconElement?.textContent).toBe('🔍');
  });

  it('adds margin below icon', () => {
    const { container } = render(<EmptyState icon="📈" />);
    const iconElement = container.querySelector('.text-4xl') as HTMLElement | null;
    expect(iconElement).not.toBeNull();
    expect(iconElement).toHaveStyle({ marginBottom: spacing.gap.xs });
  });
});
