/**
 * Unit tests for StatusDot component.
 */

import { describe, it, expect } from 'vitest';
import { render } from '@testing-library/react';
import StatusDot from '../../components/ui/badge/StatusDot';
import type { StatusDotState, StatusDotSize } from '../../components/ui/badge/StatusDot';

describe('StatusDot', () => {
  describe('Status States', () => {
    it('renders live status with emerald color and pulse', () => {
      const { container } = render(
        <StatusDot status="live" />
      );

      const dot = container.querySelector('span');
      expect(dot?.getAttribute('style')).toContain('47, 155, 116'); // updated success color
      expect(dot).toHaveClass('animate-pulse');
    });

    it('renders stale status with red color and no pulse', () => {
      const { container } = render(
        <StatusDot status="stale" />
      );

      const dot = container.querySelector('span');
      expect(dot?.getAttribute('style')).toContain('198, 76, 88');
      expect(dot).not.toHaveClass('animate-pulse');
    });

    it('renders loading status with gray color and pulse', () => {
      const { container } = render(
        <StatusDot status="loading" />
      );

      const dot = container.querySelector('span');
      expect(dot?.getAttribute('style')).toContain('128, 131, 139');
      expect(dot).toHaveClass('animate-pulse');
    });
  });

  describe('Sizes', () => {
    it('renders xs size (6px)', () => {
      const { container } = render(
        <StatusDot status="live" size="xs" />
      );

      const dot = container.querySelector('span');
      expect(dot).toHaveClass('w-1.5');
      expect(dot).toHaveClass('h-1.5');
    });

    it('renders sm size (8px) - default', () => {
      const { container } = render(
        <StatusDot status="live" size="sm" />
      );

      const dot = container.querySelector('span');
      expect(dot).toHaveClass('w-2');
      expect(dot).toHaveClass('h-2');
    });

    it('defaults to sm size when size prop omitted', () => {
      const { container } = render(
        <StatusDot status="live" />
      );

      const dot = container.querySelector('span');
      expect(dot).toHaveClass('w-2');
      expect(dot).toHaveClass('h-2');
    });

    it('renders md size (10px)', () => {
      const { container } = render(
        <StatusDot status="live" size="md" />
      );

      const dot = container.querySelector('span');
      expect(dot).toHaveClass('w-2.5');
      expect(dot).toHaveClass('h-2.5');
    });
  });

  describe('Pulse Animation', () => {
    it('pulses by default for live status', () => {
      const { container } = render(
        <StatusDot status="live" />
      );

      const dot = container.querySelector('span');
      expect(dot).toHaveClass('animate-pulse');
    });

    it('does not pulse by default for stale status', () => {
      const { container } = render(
        <StatusDot status="stale" />
      );

      const dot = container.querySelector('span');
      expect(dot).not.toHaveClass('animate-pulse');
    });

    it('pulses by default for loading status', () => {
      const { container } = render(
        <StatusDot status="loading" />
      );

      const dot = container.querySelector('span');
      expect(dot).toHaveClass('animate-pulse');
    });

    it('forces pulse when pulse=true', () => {
      const { container } = render(
        <StatusDot status="stale" pulse={true} />
      );

      const dot = container.querySelector('span');
      expect(dot).toHaveClass('animate-pulse');
    });

    it('disables pulse when pulse=false', () => {
      const { container } = render(
        <StatusDot status="live" pulse={false} />
      );

      const dot = container.querySelector('span');
      expect(dot).not.toHaveClass('animate-pulse');
    });
  });

  describe('Base Styles', () => {
    it('applies inline-block layout', () => {
      const { container } = render(
        <StatusDot status="live" />
      );

      const dot = container.querySelector('span');
      expect(dot).toHaveClass('inline-block');
    });

    it('applies rounded-full shape', () => {
      const { container } = render(
        <StatusDot status="live" />
      );

      const dot = container.querySelector('span');
      expect(dot).toHaveClass('rounded-full');
    });
  });

  describe('Custom ClassName', () => {
    it('merges custom className with base classes', () => {
      const { container } = render(
        <StatusDot status="live" className="custom-class" />
      );

      const dot = container.querySelector('span');
      expect(dot).toHaveClass('custom-class');
      expect(dot).toHaveClass('inline-block'); // Base class preserved
    });

    it('allows overriding specific styles', () => {
      const { container } = render(
        <StatusDot status="live" className="w-4 h-4" />
      );

      const dot = container.querySelector('span');
      expect(dot).toHaveClass('w-4');
      expect(dot).toHaveClass('h-4');
    });
  });

  describe('Accessibility', () => {
    it('renders as span element with role=status', () => {
      const { container } = render(
        <StatusDot status="live" />
      );

      const dot = container.querySelector('span');
      expect(dot?.tagName).toBe('SPAN');
      expect(dot).toHaveAttribute('role', 'status');
    });

    it('applies default aria-label based on status', () => {
      const { container } = render(
        <StatusDot status="live" />
      );

      const dot = container.querySelector('span');
      expect(dot).toHaveAttribute('aria-label', 'Status: live');
    });

    it('applies custom aria-label when provided', () => {
      const { container } = render(
        <StatusDot status="live" aria-label="Connection active" />
      );

      const dot = container.querySelector('span');
      expect(dot).toHaveAttribute('aria-label', 'Connection active');
    });

    it('sets aria-live=polite for live status', () => {
      const { container } = render(
        <StatusDot status="live" />
      );

      const dot = container.querySelector('span');
      expect(dot).toHaveAttribute('aria-live', 'polite');
    });

    it('sets aria-live=off for stale status', () => {
      const { container } = render(
        <StatusDot status="stale" />
      );

      const dot = container.querySelector('span');
      expect(dot).toHaveAttribute('aria-live', 'off');
    });

    it('sets aria-live=off for loading status', () => {
      const { container } = render(
        <StatusDot status="loading" />
      );

      const dot = container.querySelector('span');
      expect(dot).toHaveAttribute('aria-live', 'off');
    });
  });

  describe('All Status and Size Combinations', () => {
    const statuses: StatusDotState[] = ['live', 'stale', 'loading'];
    const sizes: StatusDotSize[] = ['xs', 'sm', 'md'];

    statuses.forEach((status) => {
      sizes.forEach((size) => {
        it(`renders ${status} status with ${size} size`, () => {
          const { container } = render(
            <StatusDot status={status} size={size} />
          );

          const dot = container.querySelector('span');
          expect(dot).toBeInTheDocument();
          expect(dot).toHaveAttribute('role', 'status');
        });
      });
    });
  });

  describe('Pulse Override Combinations', () => {
    it('live with pulse=false disables animation', () => {
      const { container } = render(
        <StatusDot status="live" pulse={false} />
      );

      const dot = container.querySelector('span');
      expect(dot).not.toHaveClass('animate-pulse');
    });

    it('stale with pulse=true enables animation', () => {
      const { container } = render(
        <StatusDot status="stale" pulse={true} />
      );

      const dot = container.querySelector('span');
      expect(dot).toHaveClass('animate-pulse');
    });

    it('loading with pulse=false disables animation', () => {
      const { container } = render(
        <StatusDot status="loading" pulse={false} />
      );

      const dot = container.querySelector('span');
      expect(dot).not.toHaveClass('animate-pulse');
    });
  });
});
