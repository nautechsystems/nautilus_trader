/**
 * Toolbar Component Tests
 *
 * Tests for button group container with spacing and orientation.
 *
 * Coverage:
 * - Rendering children
 * - Horizontal and vertical orientation
 * - Spacing variants (tight, normal, loose)
 * - Accessibility attributes
 * - Custom className
 * - Ref forwarding
 *
 * Coverage target: >90%
 */

import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { Toolbar } from '../../components/ui/toolbar/Toolbar';

describe('Toolbar Component', () => {
  describe('Rendering', () => {
    it('renders with default props', () => {
      render(
        <Toolbar>
          <button>Button 1</button>
          <button>Button 2</button>
        </Toolbar>
      );

      expect(screen.getByRole('toolbar')).toBeInTheDocument();
      expect(screen.getByText('Button 1')).toBeInTheDocument();
      expect(screen.getByText('Button 2')).toBeInTheDocument();
    });

    it('renders children correctly', () => {
      render(
        <Toolbar>
          <span>Child 1</span>
          <span>Child 2</span>
          <span>Child 3</span>
        </Toolbar>
      );

      expect(screen.getByText('Child 1')).toBeInTheDocument();
      expect(screen.getByText('Child 2')).toBeInTheDocument();
      expect(screen.getByText('Child 3')).toBeInTheDocument();
    });

    it('applies custom className', () => {
      render(
        <Toolbar className="custom-toolbar">
          <button>Button</button>
        </Toolbar>
      );

      const toolbar = screen.getByRole('toolbar');
      expect(toolbar).toHaveClass('custom-toolbar');
    });

    it('forwards ref correctly', () => {
      const ref = vi.fn();
      render(
        <Toolbar ref={ref}>
          <button>Button</button>
        </Toolbar>
      );

      expect(ref).toHaveBeenCalled();
    });
  });

  describe('Orientation', () => {
    it('renders horizontal orientation by default', () => {
      render(
        <Toolbar>
          <button>Button</button>
        </Toolbar>
      );

      const toolbar = screen.getByRole('toolbar');
      expect(toolbar).toHaveClass('flex-row');
      expect(toolbar).toHaveClass('items-center');
    });

    it('renders horizontal orientation when specified', () => {
      render(
        <Toolbar orientation="horizontal">
          <button>Button</button>
        </Toolbar>
      );

      const toolbar = screen.getByRole('toolbar');
      expect(toolbar).toHaveClass('flex-row');
      expect(toolbar).toHaveClass('items-center');
    });

    it('renders vertical orientation', () => {
      render(
        <Toolbar orientation="vertical">
          <button>Button</button>
        </Toolbar>
      );

      const toolbar = screen.getByRole('toolbar');
      expect(toolbar).toHaveClass('flex-col');
      expect(toolbar).toHaveClass('items-start');
    });
  });

  describe('Spacing', () => {
    it('applies normal spacing by default', () => {
      render(
        <Toolbar>
          <button>Button</button>
        </Toolbar>
      );

      const toolbar = screen.getByRole('toolbar');
      // Normal = 10px (0.625rem)
      expect(toolbar).toHaveStyle({ gap: '0.625rem' });
    });

    it('applies tight spacing', () => {
      render(
        <Toolbar spacing="tight">
          <button>Button</button>
        </Toolbar>
      );

      const toolbar = screen.getByRole('toolbar');
      // Tight = 6px (0.375rem)
      expect(toolbar).toHaveStyle({ gap: '0.375rem' });
    });

    it('applies normal spacing', () => {
      render(
        <Toolbar spacing="normal">
          <button>Button</button>
        </Toolbar>
      );

      const toolbar = screen.getByRole('toolbar');
      // Normal = 10px (0.625rem)
      expect(toolbar).toHaveStyle({ gap: '0.625rem' });
    });

    it('applies loose spacing', () => {
      render(
        <Toolbar spacing="loose">
          <button>Button</button>
        </Toolbar>
      );

      const toolbar = screen.getByRole('toolbar');
      // Loose = 12px (0.75rem)
      expect(toolbar).toHaveStyle({ gap: '0.75rem' });
    });
  });

  describe('Accessibility', () => {
    it('has toolbar role by default', () => {
      render(
        <Toolbar>
          <button>Button</button>
        </Toolbar>
      );

      expect(screen.getByRole('toolbar')).toBeInTheDocument();
    });

    it('supports custom role', () => {
      render(
        <Toolbar role="group">
          <button>Button</button>
        </Toolbar>
      );

      expect(screen.getByRole('group')).toBeInTheDocument();
    });

    it('supports aria-label', () => {
      render(
        <Toolbar aria-label="Primary actions">
          <button>Button</button>
        </Toolbar>
      );

      const toolbar = screen.getByRole('toolbar');
      expect(toolbar).toHaveAttribute('aria-label', 'Primary actions');
    });
  });

  describe('Combination States', () => {
    it('renders vertical toolbar with tight spacing', () => {
      render(
        <Toolbar orientation="vertical" spacing="tight">
          <button>Button 1</button>
          <button>Button 2</button>
        </Toolbar>
      );

      const toolbar = screen.getByRole('toolbar');
      expect(toolbar).toHaveClass('flex-col');
      expect(toolbar).toHaveStyle({ gap: '0.375rem' });
    });

    it('renders horizontal toolbar with loose spacing and custom class', () => {
      render(
        <Toolbar
          orientation="horizontal"
          spacing="loose"
          className="custom-class"
        >
          <button>Button 1</button>
          <button>Button 2</button>
        </Toolbar>
      );

      const toolbar = screen.getByRole('toolbar');
      expect(toolbar).toHaveClass('flex-row');
      expect(toolbar).toHaveClass('custom-class');
      expect(toolbar).toHaveStyle({ gap: '0.75rem' });
    });
  });

  describe('Integration with Buttons', () => {
    it('works with multiple button types', () => {
      render(
        <Toolbar>
          <button type="button">Action</button>
          <button type="submit">Submit</button>
          <button type="reset">Reset</button>
        </Toolbar>
      );

      expect(screen.getByText('Action')).toBeInTheDocument();
      expect(screen.getByText('Submit')).toBeInTheDocument();
      expect(screen.getByText('Reset')).toBeInTheDocument();
    });

    it('preserves button click handlers', () => {
      const handleClick = vi.fn();
      render(
        <Toolbar>
          <button onClick={handleClick}>Click me</button>
        </Toolbar>
      );

      screen.getByText('Click me').click();
      expect(handleClick).toHaveBeenCalledTimes(1);
    });
  });
});
