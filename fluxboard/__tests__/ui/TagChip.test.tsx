/**
 * Unit tests for TagChip component.
 */

import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import TagChip from '../../components/ui/badge/TagChip';
import type { BadgeVariant, BadgeSize } from '../../components/ui/badge/Badge';

describe('TagChip', () => {
  describe('Label Rendering', () => {
    it('renders label text correctly', () => {
      render(<TagChip label="Active" />);
      expect(screen.getByText('Active')).toBeInTheDocument();
    });

    it('renders numeric label', () => {
      render(<TagChip label="42" />);
      expect(screen.getByText('42')).toBeInTheDocument();
    });

    it('renders complex label', () => {
      render(<TagChip label="Filter: USD" />);
      expect(screen.getByText('Filter: USD')).toBeInTheDocument();
    });
  });

  describe('Variants', () => {
    it('defaults to neutral variant', () => {
      const { container } = render(<TagChip label="Test" />);
      const chip = container.querySelector('span');
      expect(chip?.className).toContain('bg-[rgba(44,47,55,0.9)]');
      expect(chip?.className).toContain('text-[rgb(156,161,171)]');
    });

    it('renders success variant with emerald colors', () => {
      const { container } = render(<TagChip label="Active" variant="success" />);
      const chip = container.querySelector('span');
      expect(chip?.className).toContain('bg-[rgba(15,143,107,0.14)]');
      expect(chip?.className).toContain('text-[rgb(47,180,138)]');
    });

    it('renders danger variant with red colors', () => {
      const { container } = render(<TagChip label="Error" variant="danger" />);
      const chip = container.querySelector('span');
      expect(chip?.className).toContain('bg-[rgba(224,75,73,0.14)]');
      expect(chip?.className).toContain('text-[rgb(240,112,112)]');
    });

    it('renders warning variant with amber colors', () => {
      const { container } = render(<TagChip label="Warning" variant="warning" />);
      const chip = container.querySelector('span');
      expect(chip?.className).toContain('bg-[rgba(201,154,46,0.16)]');
      expect(chip?.className).toContain('text-[rgb(221,181,80)]');
    });

    it('renders info variant with blue colors', () => {
      const { container } = render(<TagChip label="Info" variant="info" />);
      const chip = container.querySelector('span');
      expect(chip?.className).toContain('bg-[rgba(76,122,214,0.14)]');
      expect(chip?.className).toContain('text-[rgb(111,146,222)]');
    });
  });

  describe('Sizes', () => {
    it('defaults to sm size', () => {
      const { container } = render(<TagChip label="Test" />);
      const chip = container.querySelector('span');
      expect(chip).toHaveClass('text-xs');
      expect(chip).toHaveClass('px-2');
      expect(chip).toHaveClass('py-1');
      expect(chip).toHaveClass('gap-1.5');
    });

    it('renders xs size with correct spacing', () => {
      const { container } = render(<TagChip label="Test" size="xs" />);
      const chip = container.querySelector('span');
      expect(chip).toHaveClass('text-2xs');
      expect(chip).toHaveClass('px-1.5');
      expect(chip).toHaveClass('py-0.5');
      expect(chip).toHaveClass('gap-1');
    });

    it('renders md size with correct spacing', () => {
      const { container } = render(<TagChip label="Test" size="md" />);
      const chip = container.querySelector('span');
      expect(chip).toHaveClass('text-sm');
      expect(chip).toHaveClass('px-2.5');
      expect(chip).toHaveClass('py-1');
      expect(chip).toHaveClass('gap-1.5');
    });
  });

  describe('Remove Button', () => {
    it('does not render remove button when onRemove not provided', () => {
      const { container } = render(<TagChip label="Test" />);
      const button = container.querySelector('button');
      expect(button).not.toBeInTheDocument();
    });

    it('renders remove button when onRemove provided', () => {
      const { container } = render(<TagChip label="Test" onRemove={() => {}} />);
      const button = container.querySelector('button');
      expect(button).toBeInTheDocument();
    });

    it('calls onRemove when remove button clicked', () => {
      const onRemove = vi.fn();
      const { container } = render(<TagChip label="Test" onRemove={onRemove} />);

      const button = container.querySelector('button');
      fireEvent.click(button!);

      expect(onRemove).toHaveBeenCalledTimes(1);
    });

    it('does not call onRemove when chip clicked (only button)', () => {
      const onRemove = vi.fn();
      const { container } = render(<TagChip label="Test" onRemove={onRemove} />);

      const chip = container.querySelector('span');
      fireEvent.click(chip!);

      // onRemove should not be called when clicking the chip itself
      // (only when clicking the button)
      expect(onRemove).not.toHaveBeenCalled();
    });

    it('remove button has proper aria-label', () => {
      const { container } = render(<TagChip label="Test Filter" onRemove={() => {}} />);
      const button = container.querySelector('button');
      expect(button).toHaveAttribute('aria-label', 'Remove Test Filter');
    });

    it('remove button shows X icon', () => {
      const { container } = render(<TagChip label="Test" onRemove={() => {}} />);
      const button = container.querySelector('button');
      const svg = button?.querySelector('svg');
      expect(svg).toBeInTheDocument();
    });
  });

  describe('Remove Button Sizes', () => {
    it('xs size has correct button dimensions', () => {
      const { container } = render(
        <TagChip label="Test" size="xs" onRemove={() => {}} />
      );
      const button = container.querySelector('button');
      expect(button).toHaveClass('w-2.5');
      expect(button).toHaveClass('h-2.5');
    });

    it('sm size has correct button dimensions', () => {
      const { container } = render(
        <TagChip label="Test" size="sm" onRemove={() => {}} />
      );
      const button = container.querySelector('button');
      expect(button).toHaveClass('w-3');
      expect(button).toHaveClass('h-3');
    });

    it('md size has correct button dimensions', () => {
      const { container } = render(
        <TagChip label="Test" size="md" onRemove={() => {}} />
      );
      const button = container.querySelector('button');
      expect(button).toHaveClass('w-3.5');
      expect(button).toHaveClass('h-3.5');
    });
  });

  describe('Base Styles', () => {
    it('applies inline-flex layout', () => {
      const { container } = render(<TagChip label="Test" />);
      const chip = container.querySelector('span');
      expect(chip).toHaveClass('inline-flex');
      expect(chip).toHaveClass('items-center');
      expect(chip).toHaveClass('justify-center');
    });

    it('applies rounded corners', () => {
      const { container } = render(<TagChip label="Test" />);
      const chip = container.querySelector('span');
      expect(chip).toHaveClass('rounded-md');
    });

    it('applies font styling', () => {
      const { container } = render(<TagChip label="Test" />);
      const chip = container.querySelector('span');
      expect(chip).toHaveClass('font-medium');
      expect(chip).toHaveClass('tracking-tight');
      expect(chip).toHaveClass('uppercase');
    });

    it('applies whitespace-nowrap', () => {
      const { container } = render(<TagChip label="Test" />);
      const chip = container.querySelector('span');
      expect(chip).toHaveClass('whitespace-nowrap');
    });
  });

  describe('Custom ClassName', () => {
    it('merges custom className with base classes', () => {
      const { container } = render(
        <TagChip label="Test" className="custom-class" />
      );
      const chip = container.querySelector('span');
      expect(chip).toHaveClass('custom-class');
      expect(chip).toHaveClass('inline-flex'); // Base class preserved
    });

    it('allows overriding specific styles', () => {
      const { container } = render(
        <TagChip label="Test" className="px-8" />
      );
      const chip = container.querySelector('span');
      expect(chip).toHaveClass('px-8');
    });
  });

  describe('Accessibility', () => {
    it('renders as span element', () => {
      const { container } = render(<TagChip label="Test" />);
      const chip = container.querySelector('span');
      expect(chip?.tagName).toBe('SPAN');
    });

    it('applies default aria-label matching label', () => {
      const { container } = render(<TagChip label="Test Filter" />);
      const chip = container.querySelector('span');
      expect(chip).toHaveAttribute('aria-label', 'Test Filter');
    });

    it('applies custom aria-label when provided', () => {
      const { container } = render(
        <TagChip label="USD" aria-label="Currency filter: USD" />
      );
      const chip = container.querySelector('span');
      expect(chip).toHaveAttribute('aria-label', 'Currency filter: USD');
    });

    it('remove button has type=button', () => {
      const { container } = render(<TagChip label="Test" onRemove={() => {}} />);
      const button = container.querySelector('button');
      expect(button).toHaveAttribute('type', 'button');
    });

    it('remove button has focus styles', () => {
      const { container } = render(<TagChip label="Test" onRemove={() => {}} />);
      const button = container.querySelector('button');
      expect(button).toHaveClass('focus:outline-none');
      expect(button).toHaveClass('focus:ring-1');
      expect(button).toHaveClass('focus:ring-current');
    });
  });

  describe('All Variant and Size Combinations', () => {
    const variants: BadgeVariant[] = ['success', 'danger', 'warning', 'info', 'neutral'];
    const sizes: BadgeSize[] = ['xs', 'sm', 'md'];

    variants.forEach((variant) => {
      sizes.forEach((size) => {
        it(`renders ${variant} variant with ${size} size`, () => {
          const { container } = render(
            <TagChip label="Test" variant={variant} size={size} />
          );
          const chip = container.querySelector('span');
          expect(chip).toBeInTheDocument();
          expect(chip).toHaveTextContent('Test');
        });
      });
    });
  });

  describe('Remove Button Interaction', () => {
    it('multiple clicks call onRemove multiple times', () => {
      const onRemove = vi.fn();
      const { container } = render(<TagChip label="Test" onRemove={onRemove} />);

      const button = container.querySelector('button');
      fireEvent.click(button!);
      fireEvent.click(button!);
      fireEvent.click(button!);

      expect(onRemove).toHaveBeenCalledTimes(3);
    });

    it('remove button has hover opacity transition', () => {
      const { container } = render(<TagChip label="Test" onRemove={() => {}} />);
      const button = container.querySelector('button');
      expect(button).toHaveClass('transition-opacity');
      expect(button).toHaveClass('hover:opacity-70');
    });
  });

  describe('Edge Cases', () => {
    it('handles empty label', () => {
      const { container } = render(<TagChip label="" />);
      const chip = container.querySelector('span');
      expect(chip).toBeInTheDocument();
    });

    it('handles very long label', () => {
      const longLabel = 'VeryLongLabelWithoutSpacesThatShouldNotWrap';
      const { container } = render(<TagChip label={longLabel} />);
      const chip = container.querySelector('span');
      expect(chip).toHaveClass('whitespace-nowrap');
      expect(screen.getByText(longLabel)).toBeInTheDocument();
    });

    it('handles special characters in label', () => {
      render(<TagChip label="Filter: $USD > 100" />);
      expect(screen.getByText('Filter: $USD > 100')).toBeInTheDocument();
    });
  });
});
