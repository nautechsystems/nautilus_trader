/**
 * IconButton Component Tests
 *
 * Comprehensive tests for the IconButton component covering:
 * - All 4 variants (primary, secondary, danger, ghost)
 * - All 4 sizes (xs, sm, md, lg)
 * - Square aspect ratio verification
 * - Disabled state behavior
 * - Loading state behavior
 * - Keyboard activation
 * - Accessibility (required aria-label)
 *
 * Coverage target: >90%
 */

import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { IconButton } from '../../components/ui/button/IconButton';

// Simple icon component for testing
const TestIcon = () => <svg data-testid="test-icon" />;
const CloseIcon = () => <svg data-testid="close-icon" />;

describe('IconButton Component', () => {
  describe('Rendering', () => {
    it('renders with required aria-label', () => {
      render(
        <IconButton aria-label="Close dialog">
          <CloseIcon />
        </IconButton>
      );
      const button = screen.getByRole('button', { name: /close dialog/i });
      expect(button).toBeInTheDocument();
      expect(screen.getByTestId('close-icon')).toBeInTheDocument();
    });

    it('renders icon correctly', () => {
      render(
        <IconButton aria-label="Test button">
          <TestIcon />
        </IconButton>
      );
      expect(screen.getByTestId('test-icon')).toBeInTheDocument();
    });

    it('applies custom className', () => {
      render(
        <IconButton aria-label="Custom" className="custom-class">
          <TestIcon />
        </IconButton>
      );
      const button = screen.getByRole('button');
      expect(button).toHaveClass('custom-class');
    });

    it('forwards ref correctly', () => {
      const ref = vi.fn();
      render(
        <IconButton ref={ref} aria-label="Ref test">
          <TestIcon />
        </IconButton>
      );
      expect(ref).toHaveBeenCalled();
    });

    it('defaults to button type', () => {
      render(
        <IconButton aria-label="Type test">
          <TestIcon />
        </IconButton>
      );
      expect(screen.getByRole('button')).toHaveAttribute('type', 'button');
    });
  });

  describe('Variants', () => {
    it('renders primary variant with correct styles', () => {
      render(
        <IconButton variant="default" aria-label="Primary">
          <TestIcon />
        </IconButton>
      );
      const button = screen.getByRole('button');
      expect(button).toHaveClass('bg-bg-hover');
      expect(button).toHaveClass('border');
    });

    it('renders secondary variant with correct styles', () => {
      render(
        <IconButton variant="secondary" aria-label="Secondary">
          <TestIcon />
        </IconButton>
      );
      const button = screen.getByRole('button');
      expect(button).toHaveClass('bg-bg-surface');
      expect(button).toHaveClass('text-text-secondary');
    });

    it('renders danger variant with correct styles', () => {
      render(
        <IconButton variant="destructive" aria-label="Danger">
          <TestIcon />
        </IconButton>
      );
      const button = screen.getByRole('button');
      expect(button).toHaveClass('bg-danger');
      expect(button).toHaveClass('text-bg-base');
    });

    it('renders ghost variant with correct styles', () => {
      render(
        <IconButton variant="ghost" aria-label="Ghost">
          <TestIcon />
        </IconButton>
      );
      const button = screen.getByRole('button');
      expect(button).toHaveClass('hover:bg-bg-hover');
    });

    it('uses secondary as default variant', () => {
      render(
        <IconButton aria-label="Default">
          <TestIcon />
        </IconButton>
      );
      const button = screen.getByRole('button');
      expect(button).toHaveClass('bg-bg-hover');
    });
  });

  describe('Sizes (Square Aspect Ratio)', () => {
    it('renders xs size (28px square)', () => {
      render(
        <IconButton size="xs" aria-label="Extra small">
          <TestIcon />
        </IconButton>
      );
      const button = screen.getByRole('button');
      expect(button).toHaveClass('h-7');
      expect(button).toHaveClass('w-7');
      expect(button).toHaveClass('text-[11px]');
    });

    it('renders sm size (32px square)', () => {
      render(
        <IconButton size="sm" aria-label="Small">
          <TestIcon />
        </IconButton>
      );
      const button = screen.getByRole('button');
      expect(button).toHaveClass('h-8');
      expect(button).toHaveClass('w-8');
      expect(button).toHaveClass('text-[12px]');
    });

    it('renders md size (36px square)', () => {
      render(
        <IconButton size="md" aria-label="Medium">
          <TestIcon />
        </IconButton>
      );
      const button = screen.getByRole('button');
      expect(button).toHaveClass('h-9');
      expect(button).toHaveClass('w-9');
      expect(button).toHaveClass('text-[13px]');
    });

    it('renders lg size (40px square)', () => {
      render(
        <IconButton size="lg" aria-label="Large">
          <TestIcon />
        </IconButton>
      );
      const button = screen.getByRole('button');
      expect(button).toHaveClass('h-10');
      expect(button).toHaveClass('w-10');
      expect(button).toHaveClass('text-[14px]');
    });

    it('uses md as default size', () => {
      render(
        <IconButton aria-label="Default size">
          <TestIcon />
        </IconButton>
      );
      const button = screen.getByRole('button');
      expect(button).toHaveClass('h-9');
      expect(button).toHaveClass('w-9');
    });

    it('applies shrink-0 for consistent sizing', () => {
      render(
        <IconButton aria-label="No shrink">
          <TestIcon />
        </IconButton>
      );
      const button = screen.getByRole('button');
      expect(button).toHaveClass('shrink-0');
    });
  });

  describe('Disabled State', () => {
    it('renders disabled button correctly', () => {
      render(
        <IconButton disabled aria-label="Disabled">
          <TestIcon />
        </IconButton>
      );
      const button = screen.getByRole('button');
      expect(button).toBeDisabled();
      expect(button).toHaveAttribute('aria-disabled', 'true');
    });

    it('applies disabled styling', () => {
      render(
        <IconButton disabled aria-label="Disabled">
          <TestIcon />
        </IconButton>
      );
      const button = screen.getByRole('button');
      expect(button).toHaveClass('disabled:pointer-events-none');
      expect(button).toHaveClass('disabled:opacity-60');
    });

    it('prevents onClick when disabled', () => {
      const handleClick = vi.fn();
      render(
        <IconButton disabled onClick={handleClick} aria-label="Disabled">
          <TestIcon />
        </IconButton>
      );
      const button = screen.getByRole('button');
      fireEvent.click(button);
      expect(handleClick).not.toHaveBeenCalled();
    });

    it('prevents keyboard activation when disabled', () => {
      const handleClick = vi.fn();
      render(
        <IconButton disabled onClick={handleClick} aria-label="Disabled">
          <TestIcon />
        </IconButton>
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
      render(
        <IconButton loading aria-label="Loading">
          <TestIcon />
        </IconButton>
      );
      const spinner = screen.getByRole('button').querySelector('svg');
      expect(spinner).toBeInTheDocument();
      expect(spinner).toHaveClass('animate-spin');
    });

    it('hides icon when loading', () => {
      render(
        <IconButton loading aria-label="Loading">
          <TestIcon />
        </IconButton>
      );
      expect(screen.queryByTestId('test-icon')).not.toBeInTheDocument();
    });

    it('disables button when loading', () => {
      render(
        <IconButton loading aria-label="Loading">
          <TestIcon />
        </IconButton>
      );
      const button = screen.getByRole('button');
      expect(button).toBeDisabled();
      expect(button).toHaveAttribute('aria-busy', 'true');
    });

    it('prevents onClick when loading', () => {
      const handleClick = vi.fn();
      render(
        <IconButton loading onClick={handleClick} aria-label="Loading">
          <TestIcon />
        </IconButton>
      );
      const button = screen.getByRole('button');
      fireEvent.click(button);
      expect(handleClick).not.toHaveBeenCalled();
    });

    it('prevents keyboard activation when loading', () => {
      const handleClick = vi.fn();
      render(
        <IconButton loading onClick={handleClick} aria-label="Loading">
          <TestIcon />
        </IconButton>
      );
      const button = screen.getByRole('button');

      fireEvent.keyDown(button, { key: 'Enter' });
      expect(handleClick).not.toHaveBeenCalled();

      fireEvent.keyDown(button, { key: ' ' });
      expect(handleClick).not.toHaveBeenCalled();
    });

    it('renders correct spinner size for xs', () => {
      render(
        <IconButton size="xs" loading aria-label="Loading">
          <TestIcon />
        </IconButton>
      );
      const spinner = screen.getByRole('button').querySelector('svg');
      expect(spinner).toHaveClass('h-3');
      expect(spinner).toHaveClass('w-3');
    });

    it('renders correct spinner size for sm', () => {
      render(
        <IconButton size="sm" loading aria-label="Loading">
          <TestIcon />
        </IconButton>
      );
      const spinner = screen.getByRole('button').querySelector('svg');
      expect(spinner).toHaveClass('h-3.5');
      expect(spinner).toHaveClass('w-3.5');
    });

    it('renders correct spinner size for md', () => {
      render(
        <IconButton size="md" loading aria-label="Loading">
          <TestIcon />
        </IconButton>
      );
      const spinner = screen.getByRole('button').querySelector('svg');
      expect(spinner).toHaveClass('h-4');
      expect(spinner).toHaveClass('w-4');
    });

    it('renders correct spinner size for lg', () => {
      render(
        <IconButton size="lg" loading aria-label="Loading">
          <TestIcon />
        </IconButton>
      );
      const spinner = screen.getByRole('button').querySelector('svg');
      expect(spinner).toHaveClass('h-5');
      expect(spinner).toHaveClass('w-5');
    });
  });

  describe('Click Handling', () => {
    it('calls onClick when clicked', () => {
      const handleClick = vi.fn();
      render(
        <IconButton onClick={handleClick} aria-label="Click me">
          <TestIcon />
        </IconButton>
      );
      const button = screen.getByRole('button');
      fireEvent.click(button);
      expect(handleClick).toHaveBeenCalledTimes(1);
    });

    it('passes event to onClick handler', () => {
      const handleClick = vi.fn();
      render(
        <IconButton onClick={handleClick} aria-label="Click me">
          <TestIcon />
        </IconButton>
      );
      const button = screen.getByRole('button');
      fireEvent.click(button);
      expect(handleClick).toHaveBeenCalledWith(expect.any(Object));
    });
  });

  describe('Keyboard Activation', () => {
    it('activates on Enter key', () => {
      const handleClick = vi.fn();
      render(
        <IconButton onClick={handleClick} aria-label="Activate">
          <TestIcon />
        </IconButton>
      );
      const button = screen.getByRole('button');
      fireEvent.keyDown(button, { key: 'Enter' });
      expect(handleClick).toHaveBeenCalledTimes(1);
    });

    it('activates on Space key', () => {
      const handleClick = vi.fn();
      render(
        <IconButton onClick={handleClick} aria-label="Activate">
          <TestIcon />
        </IconButton>
      );
      const button = screen.getByRole('button');
      fireEvent.keyDown(button, { key: ' ' });
      expect(handleClick).toHaveBeenCalledTimes(1);
    });

    it('does not activate on other keys', () => {
      const handleClick = vi.fn();
      render(
        <IconButton onClick={handleClick} aria-label="No activate">
          <TestIcon />
        </IconButton>
      );
      const button = screen.getByRole('button');

      fireEvent.keyDown(button, { key: 'a' });
      fireEvent.keyDown(button, { key: 'Escape' });
      fireEvent.keyDown(button, { key: 'Tab' });

      expect(handleClick).not.toHaveBeenCalled();
    });

    it('preserves onKeyDown prop', () => {
      const handleKeyDown = vi.fn();
      render(
        <IconButton onKeyDown={handleKeyDown} aria-label="Keyboard">
          <TestIcon />
        </IconButton>
      );
      const button = screen.getByRole('button');
      fireEvent.keyDown(button, { key: 'a' });
      expect(handleKeyDown).toHaveBeenCalled();
    });
  });

  describe('Accessibility', () => {
    it('has correct role', () => {
      render(
        <IconButton aria-label="Accessible">
          <TestIcon />
        </IconButton>
      );
      expect(screen.getByRole('button')).toBeInTheDocument();
    });

    it('requires aria-label prop', () => {
      render(
        <IconButton aria-label="Required label">
          <TestIcon />
        </IconButton>
      );
      const button = screen.getByRole('button');
      expect(button).toHaveAttribute('aria-label', 'Required label');
    });

    it('sets aria-disabled when disabled', () => {
      render(
        <IconButton disabled aria-label="Disabled">
          <TestIcon />
        </IconButton>
      );
      expect(screen.getByRole('button')).toHaveAttribute('aria-disabled', 'true');
    });

    it('sets aria-disabled when loading', () => {
      render(
        <IconButton loading aria-label="Loading">
          <TestIcon />
        </IconButton>
      );
      expect(screen.getByRole('button')).toHaveAttribute('aria-disabled', 'true');
    });

    it('sets aria-busy when loading', () => {
      render(
        <IconButton loading aria-label="Loading">
          <TestIcon />
        </IconButton>
      );
      expect(screen.getByRole('button')).toHaveAttribute('aria-busy', 'true');
    });

    it('does not set aria-busy when not loading', () => {
      render(
        <IconButton aria-label="Not loading">
          <TestIcon />
        </IconButton>
      );
      expect(screen.getByRole('button')).toHaveAttribute('aria-busy', 'false');
    });
  });

  describe('Combination States', () => {
    it('renders primary large icon button', () => {
      render(
        <IconButton variant="default" size="lg" aria-label="Primary large">
          <TestIcon />
        </IconButton>
      );
      const button = screen.getByRole('button');
      expect(button).toHaveClass('bg-bg-hover');
      expect(button).toHaveClass('h-10');
      expect(button).toHaveClass('w-10');
    });

    it('renders danger small loading icon button', () => {
      render(
        <IconButton variant="destructive" size="sm" loading aria-label="Deleting">
          <TestIcon />
        </IconButton>
      );
      const button = screen.getByRole('button');
      expect(button).toHaveClass('bg-danger');
      expect(button).toHaveClass('h-8');
      expect(button).toHaveClass('w-8');
      expect(button).toBeDisabled();
    });

    it('renders ghost extra small disabled icon button', () => {
      render(
        <IconButton variant="ghost" size="xs" disabled aria-label="Disabled ghost">
          <TestIcon />
        </IconButton>
      );
      const button = screen.getByRole('button');
      expect(button).toHaveClass('hover:bg-bg-hover');
      expect(button).toHaveClass('h-7');
      expect(button).toHaveClass('w-7');
      expect(button).toBeDisabled();
    });
  });
});
