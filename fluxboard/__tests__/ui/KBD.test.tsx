/**
 * KBD Component Tests
 *
 * Tests for keyboard shortcut hint display.
 *
 * Coverage:
 * - Rendering children (shortcut text)
 * - Styling (monospace, border, padding)
 * - Custom className
 * - Ref forwarding
 * - Various shortcut formats
 *
 * Coverage target: >90%
 */

import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { KBD } from '../../components/ui/kbd/KBD';

describe('KBD Component', () => {
  describe('Rendering', () => {
    it('renders with default props', () => {
      render(<KBD>Enter</KBD>);
      expect(screen.getByText('Enter')).toBeInTheDocument();
    });

    it('renders children correctly', () => {
      render(<KBD>⌘K</KBD>);
      expect(screen.getByText('⌘K')).toBeInTheDocument();
    });

    it('renders as kbd element', () => {
      const { container } = render(<KBD>Ctrl+S</KBD>);
      const kbdElement = container.querySelector('kbd');
      expect(kbdElement).toBeInTheDocument();
      expect(kbdElement).toHaveTextContent('Ctrl+S');
    });

    it('applies custom className', () => {
      const { container } = render(
        <KBD className="custom-kbd">Esc</KBD>
      );
      const kbdElement = container.querySelector('kbd');
      expect(kbdElement).toHaveClass('custom-kbd');
    });

    it('forwards ref correctly', () => {
      const ref = vi.fn();
      render(<KBD ref={ref}>Space</KBD>);
      expect(ref).toHaveBeenCalled();
    });
  });

  describe('Styling', () => {
    it('has monospace font', () => {
      const { container } = render(<KBD>Enter</KBD>);
      const kbdElement = container.querySelector('kbd');
      expect(kbdElement).toHaveClass('font-mono');
    });

    it('has correct text size', () => {
      const { container } = render(<KBD>Enter</KBD>);
      const kbdElement = container.querySelector('kbd');
      expect(kbdElement).toHaveClass('text-xs');
    });

    it('has correct padding', () => {
      const { container } = render(<KBD>Enter</KBD>);
      const kbdElement = container.querySelector('kbd');
      expect(kbdElement).toHaveClass('px-1.5');
      expect(kbdElement).toHaveClass('py-0.5');
    });

    it('has rounded corners', () => {
      const { container } = render(<KBD>Enter</KBD>);
      const kbdElement = container.querySelector('kbd');
      expect(kbdElement).toHaveClass('rounded');
    });

    it('has border styling', () => {
      const { container } = render(<KBD>Enter</KBD>);
      const kbdElement = container.querySelector('kbd');
      expect(kbdElement).toHaveClass('border');
      expect(kbdElement).toHaveClass('border-neutral-700');
    });

    it('has background color', () => {
      const { container } = render(<KBD>Enter</KBD>);
      const kbdElement = container.querySelector('kbd');
      expect(kbdElement).toHaveClass('bg-neutral-800');
    });

    it('has text color', () => {
      const { container } = render(<KBD>Enter</KBD>);
      const kbdElement = container.querySelector('kbd');
      expect(kbdElement).toHaveClass('text-neutral-200');
    });

    it('is inline-block', () => {
      const { container } = render(<KBD>Enter</KBD>);
      const kbdElement = container.querySelector('kbd');
      expect(kbdElement).toHaveClass('inline-block');
    });

    it('preserves whitespace', () => {
      const { container } = render(<KBD>Enter</KBD>);
      const kbdElement = container.querySelector('kbd');
      expect(kbdElement).toHaveClass('whitespace-nowrap');
    });
  });

  describe('Shortcut Formats', () => {
    it('renders simple key', () => {
      render(<KBD>Enter</KBD>);
      expect(screen.getByText('Enter')).toBeInTheDocument();
    });

    it('renders command key shortcut', () => {
      render(<KBD>⌘K</KBD>);
      expect(screen.getByText('⌘K')).toBeInTheDocument();
    });

    it('renders ctrl shortcut', () => {
      render(<KBD>Ctrl+S</KBD>);
      expect(screen.getByText('Ctrl+S')).toBeInTheDocument();
    });

    it('renders shift shortcut', () => {
      render(<KBD>Shift+Tab</KBD>);
      expect(screen.getByText('Shift+Tab')).toBeInTheDocument();
    });

    it('renders alt shortcut', () => {
      render(<KBD>Alt+F4</KBD>);
      expect(screen.getByText('Alt+F4')).toBeInTheDocument();
    });

    it('renders escape key', () => {
      render(<KBD>Esc</KBD>);
      expect(screen.getByText('Esc')).toBeInTheDocument();
    });

    it('renders space key', () => {
      render(<KBD>Space</KBD>);
      expect(screen.getByText('Space')).toBeInTheDocument();
    });

    it('renders arrow keys', () => {
      render(
        <>
          <KBD>↑</KBD>
          <KBD>↓</KBD>
          <KBD>←</KBD>
          <KBD>→</KBD>
        </>
      );
      expect(screen.getByText('↑')).toBeInTheDocument();
      expect(screen.getByText('↓')).toBeInTheDocument();
      expect(screen.getByText('←')).toBeInTheDocument();
      expect(screen.getByText('→')).toBeInTheDocument();
    });
  });

  describe('Usage in Context', () => {
    it('works inline with text', () => {
      render(
        <p>
          Press <KBD>Enter</KBD> to submit
        </p>
      );
      // Text is split across nodes, so use regex matcher
      expect(screen.getByText(/Press/)).toBeInTheDocument();
      expect(screen.getByText('Enter')).toBeInTheDocument();
      expect(screen.getByText(/to submit/)).toBeInTheDocument();
    });

    it('works with multiple shortcuts', () => {
      render(
        <div>
          <KBD>Ctrl+C</KBD>
          <span> to copy, </span>
          <KBD>Ctrl+V</KBD>
          <span> to paste</span>
        </div>
      );
      expect(screen.getByText('Ctrl+C')).toBeInTheDocument();
      expect(screen.getByText('Ctrl+V')).toBeInTheDocument();
    });
  });

  describe('Custom Props', () => {
    it('accepts custom HTML attributes', () => {
      const { container } = render(
        <KBD title="Keyboard shortcut">Enter</KBD>
      );
      const kbdElement = container.querySelector('kbd');
      expect(kbdElement).toHaveAttribute('title', 'Keyboard shortcut');
    });

    it('accepts data attributes', () => {
      const { container } = render(
        <KBD data-testid="my-kbd">Enter</KBD>
      );
      const kbdElement = container.querySelector('kbd');
      expect(kbdElement).toHaveAttribute('data-testid', 'my-kbd');
    });
  });

  describe('Edge Cases', () => {
    it('handles empty children gracefully', () => {
      const { container } = render(<KBD>{''}</KBD>);
      const kbdElement = container.querySelector('kbd');
      expect(kbdElement).toBeInTheDocument();
      expect(kbdElement).toBeEmptyDOMElement();
    });

    it('handles numeric children', () => {
      render(<KBD>{0}</KBD>);
      expect(screen.getByText('0')).toBeInTheDocument();
    });

    it('handles complex ReactNode children', () => {
      render(
        <KBD>
          <span>Ctrl</span>+<span>S</span>
        </KBD>
      );
      expect(screen.getByText('Ctrl')).toBeInTheDocument();
      expect(screen.getByText('S')).toBeInTheDocument();
    });
  });
});
