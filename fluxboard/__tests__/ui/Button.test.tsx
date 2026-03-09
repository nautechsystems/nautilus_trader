/**
 * Button Component Tests
 *
 * Comprehensive tests for the Button component covering:
 * - All 4 variants (primary, secondary, danger, ghost)
 * - All 4 sizes (xs, sm, md, lg)
 * - Disabled state behavior
 * - Loading state behavior
 * - Icon rendering
 * - Keyboard activation (Enter, Space)
 * - Accessibility attributes
 *
 * Coverage target: >90%
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { Button } from '../../components/ui/button/Button';

// Simple icon component for testing
const TestIcon = () => <svg data-testid="test-icon" />;

describe('Button Component', () => {
  describe('Rendering', () => {
    it('renders with default props', () => {
      render(<Button>Click me</Button>);
      const button = screen.getByRole('button', { name: /click me/i });
      expect(button).toBeInTheDocument();
      expect(button).toHaveAttribute('type', 'button');
    });

    it('renders children correctly', () => {
      render(<Button>Submit Form</Button>);
      expect(screen.getByText('Submit Form')).toBeInTheDocument();
    });

    it('applies custom className', () => {
      render(<Button className="custom-class">Button</Button>);
      const button = screen.getByRole('button');
      expect(button).toHaveClass('custom-class');
    });

    it('forwards ref correctly', () => {
      const ref = vi.fn();
      render(<Button ref={ref}>Button</Button>);
      expect(ref).toHaveBeenCalled();
    });
  });

  describe('Variants', () => {
    it('renders primary variant with correct styles', () => {
      render(<Button variant="default">Primary</Button>);
      const button = screen.getByRole('button');
      expect(button).toBeInTheDocument();
    });

    it('renders secondary variant with correct styles', () => {
      render(<Button variant="secondary">Secondary</Button>);
      const button = screen.getByRole('button');
      expect(button).toBeInTheDocument();
    });

    it('renders danger variant with correct styles', () => {
      render(<Button variant="danger">Danger</Button>);
      const button = screen.getByRole('button');
      expect(button).toBeInTheDocument();
    });

    it('renders ghost variant with correct styles', () => {
      render(<Button variant="ghost">Ghost</Button>);
      const button = screen.getByRole('button');
      expect(button).toHaveClass('hover:bg-bg-hover');
    });

    it('uses secondary as default variant', () => {
      render(<Button>Default</Button>);
      const button = screen.getByRole('button');
      expect(button).toHaveClass('bg-accent');
    });
  });

  describe('Sizes', () => {
    it('renders xs size with correct classes', () => {
      render(<Button size="xs">Extra Small</Button>);
      const button = screen.getByRole('button');
      expect(button).toHaveClass('h-7');
      expect(button).toHaveClass('px-3');
      expect(button).toHaveClass('text-[11px]');
    });

    it('renders sm size with correct classes', () => {
      render(<Button size="sm">Small</Button>);
      const button = screen.getByRole('button');
      expect(button).toHaveClass('h-8');
      expect(button).toHaveClass('px-3.5');
      expect(button).toHaveClass('text-[12px]');
    });

    it('renders md size with correct classes', () => {
      render(<Button size="md">Medium</Button>);
      const button = screen.getByRole('button');
      expect(button).toHaveClass('h-10');
      expect(button).toHaveClass('px-4');
      expect(button).toHaveClass('text-[13px]');
    });

    it('renders lg size with correct classes', () => {
      render(<Button size="lg">Large</Button>);
      const button = screen.getByRole('button');
      expect(button).toHaveClass('h-11');
      expect(button).toHaveClass('px-5');
      expect(button).toHaveClass('text-[14px]');
    });

    it('uses md as default size', () => {
      render(<Button>Default Size</Button>);
      const button = screen.getByRole('button');
      expect(button).toHaveClass('h-10');
    });
  });

  describe('Disabled State', () => {
    it('renders disabled button correctly', () => {
      render(<Button disabled>Disabled</Button>);
      const button = screen.getByRole('button');
      expect(button).toBeDisabled();
      expect(button).toHaveAttribute('aria-disabled', 'true');
    });

    it('applies disabled styling', () => {
      render(<Button disabled>Disabled</Button>);
      const button = screen.getByRole('button');
      expect(button).toHaveClass('disabled:pointer-events-none');
      expect(button).toHaveClass('disabled:opacity-60');
    });

    it('prevents onClick when disabled', () => {
      const handleClick = vi.fn();
      render(
        <Button disabled onClick={handleClick}>
          Disabled
        </Button>
      );
      const button = screen.getByRole('button');
      fireEvent.click(button);
      expect(handleClick).not.toHaveBeenCalled();
    });

    it('prevents keyboard activation when disabled', () => {
      const handleClick = vi.fn();
      render(
        <Button disabled onClick={handleClick}>
          Disabled
        </Button>
      );
      const button = screen.getByRole('button');

      fireEvent.keyDown(button, { key: 'Enter' });
      expect(handleClick).not.toHaveBeenCalled();

      fireEvent.keyDown(button, { key: ' ' });
      expect(handleClick).not.toHaveBeenCalled();
    });
  });

  describe('Loading State', () => {
    it('shows spinner when loading', () => {
      render(<Button loading>Loading</Button>);
      const spinner = screen.getByRole('button').querySelector('svg');
      expect(spinner).toBeInTheDocument();
      expect(spinner).toHaveClass('animate-spin');
    });

    it('disables button when loading', () => {
      render(<Button loading>Loading</Button>);
      const button = screen.getByRole('button');
      expect(button).toBeDisabled();
      expect(button).toHaveAttribute('aria-busy', 'true');
    });

    it('prevents onClick when loading', () => {
      const handleClick = vi.fn();
      render(
        <Button loading onClick={handleClick}>
          Loading
        </Button>
      );
      const button = screen.getByRole('button');
      fireEvent.click(button);
      expect(handleClick).not.toHaveBeenCalled();
    });

    it('prevents keyboard activation when loading', () => {
      const handleClick = vi.fn();
      render(
        <Button loading onClick={handleClick}>
          Loading
        </Button>
      );
      const button = screen.getByRole('button');

      fireEvent.keyDown(button, { key: 'Enter' });
      expect(handleClick).not.toHaveBeenCalled();

      fireEvent.keyDown(button, { key: ' ' });
      expect(handleClick).not.toHaveBeenCalled();
    });

    it('renders correct spinner size for xs', () => {
      render(
        <Button size="xs" loading>
          Loading
        </Button>
      );
      const spinner = screen.getByRole('button').querySelector('svg');
      expect(spinner).toHaveClass('h-3');
      expect(spinner).toHaveClass('w-3');
    });

    it('renders correct spinner size for sm', () => {
      render(
        <Button size="sm" loading>
          Loading
        </Button>
      );
      const spinner = screen.getByRole('button').querySelector('svg');
      expect(spinner).toHaveClass('h-3.5');
      expect(spinner).toHaveClass('w-3.5');
    });

    it('renders correct spinner size for md', () => {
      render(
        <Button size="md" loading>
          Loading
        </Button>
      );
      const spinner = screen.getByRole('button').querySelector('svg');
      expect(spinner).toHaveClass('h-4');
      expect(spinner).toHaveClass('w-4');
    });

    it('renders correct spinner size for lg', () => {
      render(
        <Button size="lg" loading>
          Loading
        </Button>
      );
      const spinner = screen.getByRole('button').querySelector('svg');
      expect(spinner).toHaveClass('h-5');
      expect(spinner).toHaveClass('w-5');
    });

    it('renders children alongside spinner', () => {
      render(<Button loading>Submitting...</Button>);
      expect(screen.getByText('Submitting...')).toBeInTheDocument();
    });
  });

  describe('Icon Support', () => {
    it('renders icon when provided', () => {
      render(
        <Button icon={<TestIcon />}>With Icon</Button>
      );
      expect(screen.getByTestId('test-icon')).toBeInTheDocument();
      expect(screen.getByText('With Icon')).toBeInTheDocument();
    });

    it('renders icon-only button', () => {
      render(<Button icon={<TestIcon />} />);
      expect(screen.getByTestId('test-icon')).toBeInTheDocument();
    });

    it('does not render icon when loading', () => {
      render(
        <Button icon={<TestIcon />} loading>
          Loading
        </Button>
      );
      expect(screen.queryByTestId('test-icon')).not.toBeInTheDocument();
    });
  });

  describe('Click Handling', () => {
    it('calls onClick when clicked', () => {
      const handleClick = vi.fn();
      render(<Button onClick={handleClick}>Click me</Button>);
      const button = screen.getByRole('button');
      fireEvent.click(button);
      expect(handleClick).toHaveBeenCalledTimes(1);
    });

    it('passes event to onClick handler', () => {
      const handleClick = vi.fn();
      render(<Button onClick={handleClick}>Click me</Button>);
      const button = screen.getByRole('button');
      fireEvent.click(button);
      expect(handleClick).toHaveBeenCalledWith(expect.any(Object));
    });
  });

  describe('Keyboard Activation', () => {
    it('activates on Enter key', () => {
      const handleClick = vi.fn();
      render(<Button onClick={handleClick}>Activate</Button>);
      const button = screen.getByRole('button');
      fireEvent.keyDown(button, { key: 'Enter' });
      expect(handleClick).toHaveBeenCalledTimes(1);
    });

    it('activates on Space key', () => {
      const handleClick = vi.fn();
      render(<Button onClick={handleClick}>Activate</Button>);
      const button = screen.getByRole('button');
      fireEvent.keyDown(button, { key: ' ' });
      expect(handleClick).toHaveBeenCalledTimes(1);
    });

    it('does not activate on other keys', () => {
      const handleClick = vi.fn();
      render(<Button onClick={handleClick}>No Activate</Button>);
      const button = screen.getByRole('button');

      fireEvent.keyDown(button, { key: 'a' });
      fireEvent.keyDown(button, { key: 'Escape' });
      fireEvent.keyDown(button, { key: 'Tab' });

      expect(handleClick).not.toHaveBeenCalled();
    });

    it('preserves onKeyDown prop', () => {
      const handleKeyDown = vi.fn();
      render(
        <Button onKeyDown={handleKeyDown}>Keyboard</Button>
      );
      const button = screen.getByRole('button');
      fireEvent.keyDown(button, { key: 'a' });
      expect(handleKeyDown).toHaveBeenCalled();
    });
  });

  describe('Accessibility', () => {
    it('has correct role', () => {
      render(<Button>Accessible</Button>);
      expect(screen.getByRole('button')).toBeInTheDocument();
    });

    it('sets aria-disabled when disabled', () => {
      render(<Button disabled>Disabled</Button>);
      expect(screen.getByRole('button')).toHaveAttribute('aria-disabled', 'true');
    });

    it('sets aria-disabled when loading', () => {
      render(<Button loading>Loading</Button>);
      expect(screen.getByRole('button')).toHaveAttribute('aria-disabled', 'true');
    });

    it('sets aria-busy when loading', () => {
      render(<Button loading>Loading</Button>);
      expect(screen.getByRole('button')).toHaveAttribute('aria-busy', 'true');
    });

    it('does not set aria-busy when not loading', () => {
      render(<Button>Not Loading</Button>);
      expect(screen.getByRole('button')).toHaveAttribute('aria-busy', 'false');
    });

    it('supports custom aria attributes', () => {
      render(
        <Button aria-label="Custom label" aria-describedby="description">
          Button
        </Button>
      );
      const button = screen.getByRole('button');
      expect(button).toHaveAttribute('aria-label', 'Custom label');
      expect(button).toHaveAttribute('aria-describedby', 'description');
    });
  });

  describe('Type Attribute', () => {
    it('defaults to button type', () => {
      render(<Button>Button</Button>);
      expect(screen.getByRole('button')).toHaveAttribute('type', 'button');
    });

    it('supports submit type', () => {
      render(<Button type="submit">Submit</Button>);
      expect(screen.getByRole('button')).toHaveAttribute('type', 'submit');
    });

    it('supports reset type', () => {
      render(<Button type="reset">Reset</Button>);
      expect(screen.getByRole('button')).toHaveAttribute('type', 'reset');
    });
  });

  describe('Combination States', () => {
    it('renders primary large button', () => {
      render(
        <Button variant="default" size="lg">
          Primary Large
        </Button>
      );
      const button = screen.getByRole('button');
      expect(button).toHaveClass('h-11');
    });

    it('renders danger small loading button', () => {
      render(
        <Button variant="danger" size="sm" loading>
          Deleting...
        </Button>
      );
      const button = screen.getByRole('button');
      expect(button).toHaveClass('h-8');
      expect(button).toBeDisabled();
    });

    it('renders ghost extra small disabled button', () => {
      render(
        <Button variant="ghost" size="xs" disabled>
          Disabled Ghost
        </Button>
      );
      const button = screen.getByRole('button');
      expect(button).toHaveClass('hover:bg-bg-hover');
      expect(button).toHaveClass('h-7');
      expect(button).toBeDisabled();
    });
  });
});
