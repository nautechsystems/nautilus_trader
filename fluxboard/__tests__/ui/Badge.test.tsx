/**
 * Unit tests for Badge component.
 */

import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import Badge from '../../components/ui/badge/Badge';
import type { BadgeVariant, BadgeSize } from '../../components/ui/badge/Badge';

describe('Badge', () => {
  describe('Variants', () => {
    it('renders success variant with emerald colors', () => {
      const { container } = render(
        <Badge variant="success">Active</Badge>
      );

      const badge = container.querySelector('span');
      expect(badge?.getAttribute('style')).toContain('background-color: rgba(47, 155, 116, 0.12)');
      expect(badge?.getAttribute('style')).toContain('color: rgb(56, 164, 124)');
    });

    it('renders danger variant with red colors', () => {
      const { container } = render(
        <Badge variant="danger">Error</Badge>
      );

      const badge = container.querySelector('span');
      expect(badge?.getAttribute('style')).toContain('background-color: rgba(198, 76, 88, 0.12)');
      expect(badge?.getAttribute('style')).toContain('color: rgb(215, 95, 106)');
    });

    it('renders warning variant with amber colors', () => {
      const { container } = render(
        <Badge variant="warning">Warning</Badge>
      );

      const badge = container.querySelector('span');
      expect(badge?.getAttribute('style')).toContain('background-color: rgba(193, 138, 58, 0.14)');
      expect(badge?.getAttribute('style')).toContain('color: rgb(208, 154, 74)');
    });

    it('renders info variant with blue colors', () => {
      const { container } = render(
        <Badge variant="info">Info</Badge>
      );

      const badge = container.querySelector('span');
      expect(badge?.getAttribute('style')).toContain('background-color: rgba(95, 122, 195, 0.12)');
      expect(badge?.getAttribute('style')).toContain('color: rgb(122, 148, 217)');
    });

    it('renders neutral variant with gray colors', () => {
      const { container } = render(
        <Badge variant="neutral">Idle</Badge>
      );

      const badge = container.querySelector('span');
      expect(badge?.getAttribute('style')).toContain('background-color: rgb(21, 22, 24)');
      expect(badge?.getAttribute('style')).toContain('color: rgb(194, 196, 200)');
    });
  });

  describe('Sizes', () => {
    it('renders xs size with correct padding and text', () => {
      const { container } = render(
        <Badge variant="success" size="xs">XS</Badge>
      );

      const badge = container.querySelector('span');
      expect(badge?.className).toContain('text-[10px]');
      expect(badge?.className).toContain('px-[6px]');
      expect(badge?.className).toContain('py-[3px]');
    });

    it('renders sm size with correct padding and text (default)', () => {
      const { container } = render(
        <Badge variant="success" size="sm">SM</Badge>
      );

      const badge = container.querySelector('span');
      expect(badge?.className).toContain('text-[11px]');
      expect(badge?.className).toContain('px-[8px]');
      expect(badge?.className).toContain('py-[4px]');
    });

    it('defaults to sm size when size prop omitted', () => {
      const { container } = render(
        <Badge variant="success">Default</Badge>
      );

      const badge = container.querySelector('span');
      expect(badge?.className).toContain('text-[11px]');
      expect(badge?.className).toContain('px-[8px]');
      expect(badge?.className).toContain('py-[4px]');
    });

    it('renders md size with correct padding and text', () => {
      const { container } = render(
        <Badge variant="success" size="md">MD</Badge>
      );

      const badge = container.querySelector('span');
      expect(badge?.className).toContain('text-[12px]');
      expect(badge?.className).toContain('px-[10px]');
      expect(badge?.className).toContain('py-[5px]');
    });
  });

  describe('Content Rendering', () => {
    it('renders text children correctly', () => {
      render(
        <Badge variant="success">Active</Badge>
      );

      expect(screen.getByText('Active')).toBeInTheDocument();
    });

    it('renders numeric children correctly', () => {
      render(
        <Badge variant="info">{42}</Badge>
      );

      expect(screen.getByText('42')).toBeInTheDocument();
    });

    it('renders multiple text nodes', () => {
      render(
        <Badge variant="warning">
          Warning <strong>Alert</strong>
        </Badge>
      );

      expect(screen.getByText(/Warning/)).toBeInTheDocument();
      expect(screen.getByText('Alert')).toBeInTheDocument();
    });
  });

  describe('Base Styles', () => {
    it('applies inline-flex layout', () => {
      const { container } = render(
        <Badge variant="success">Test</Badge>
      );

      const badge = container.querySelector('span');
      expect(badge).toHaveClass('inline-flex');
      expect(badge).toHaveClass('items-center');
      expect(badge).toHaveClass('justify-center');
    });

    it('applies rounded corners', () => {
      const { container } = render(
        <Badge variant="success">Test</Badge>
      );

      const badge = container.querySelector('span');
      expect(badge?.className).toContain('rounded-[3px]');
    });

    it('applies font styling', () => {
      const { container } = render(
        <Badge variant="success">Test</Badge>
      );

      const badge = container.querySelector('span');
      expect(badge).toHaveClass('font-semibold');
    });

    it('applies whitespace-nowrap', () => {
      const { container } = render(
        <Badge variant="success">Test</Badge>
      );

      const badge = container.querySelector('span');
      expect(badge).toHaveClass('whitespace-nowrap');
    });
  });

  describe('Custom ClassName', () => {
    it('merges custom className with base classes', () => {
      const { container } = render(
        <Badge variant="success" className="custom-class">Test</Badge>
      );

      const badge = container.querySelector('span');
      expect(badge).toHaveClass('custom-class');
      expect(badge).toHaveClass('inline-flex'); // Base class preserved
    });

    it('allows overriding specific styles', () => {
      const { container } = render(
        <Badge variant="success" className="px-8">Test</Badge>
      );

      const badge = container.querySelector('span');
      expect(badge).toHaveClass('px-8');
    });
  });

  describe('Accessibility', () => {
    it('renders as span element', () => {
      const { container } = render(
        <Badge variant="success">Test</Badge>
      );

      const badge = container.querySelector('span');
      expect(badge?.tagName).toBe('SPAN');
    });

    it('applies aria-label when provided', () => {
      const { container } = render(
        <Badge variant="success" aria-label="Status: Active">ACT</Badge>
      );

      const badge = container.querySelector('span');
      expect(badge).toHaveAttribute('aria-label', 'Status: Active');
    });

    it('does not apply aria-label by default', () => {
      const { container } = render(
        <Badge variant="success">Test</Badge>
      );

      const badge = container.querySelector('span');
      expect(badge).not.toHaveAttribute('aria-label');
    });
  });

  describe('All Variant and Size Combinations', () => {
    const variants: BadgeVariant[] = ['success', 'danger', 'warning', 'info', 'neutral'];
    const sizes: BadgeSize[] = ['xs', 'sm', 'md'];

    variants.forEach((variant) => {
      sizes.forEach((size) => {
        it(`renders ${variant} variant with ${size} size`, () => {
          const { container } = render(
            <Badge variant={variant} size={size}>Test</Badge>
          );

          const badge = container.querySelector('span');
          expect(badge).toBeInTheDocument();
          expect(badge).toHaveTextContent('Test');
        });
      });
    });
  });

  describe('Edge Cases', () => {
    it('handles empty string children', () => {
      const { container } = render(
        <Badge variant="success">{''}</Badge>
      );

      const badge = container.querySelector('span');
      expect(badge).toBeInTheDocument();
      expect(badge).toHaveTextContent('');
    });

    it('handles zero as children', () => {
      render(
        <Badge variant="info">{0}</Badge>
      );

      expect(screen.getByText('0')).toBeInTheDocument();
    });

    it('handles long text without breaking layout', () => {
      const longText = 'VeryLongTextWithoutSpacesThatShouldNotWrap';
      const { container } = render(
        <Badge variant="success">{longText}</Badge>
      );

      const badge = container.querySelector('span');
      expect(badge).toHaveClass('whitespace-nowrap');
      expect(screen.getByText(longText)).toBeInTheDocument();
    });
  });
});
