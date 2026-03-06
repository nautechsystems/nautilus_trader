/**
 * Tests for AutoModeToggle component
 */

import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { AutoModeToggle } from '@/components/domain/params/AutoModeToggle';

describe('AutoModeToggle', () => {
  it('should render checked when auto is enabled', () => {
    render(
      <AutoModeToggle
        auto={true}
        isActive={true}
        intervalMs={3000}
        hasInputFocus={false}
        hasDirty={false}
        onToggle={vi.fn()}
      />
    );

    const checkbox = screen.getByRole('checkbox');
    expect(checkbox).toBeChecked();
    expect(screen.getByText(/auto \(3s\)/i)).toBeInTheDocument();
  });

  it('should render unchecked when auto is disabled', () => {
    render(
      <AutoModeToggle
        auto={false}
        isActive={false}
        intervalMs={3000}
        hasInputFocus={false}
        hasDirty={false}
        onToggle={vi.fn()}
      />
    );

    const checkbox = screen.getByRole('checkbox');
    expect(checkbox).not.toBeChecked();
  });

  it('should show paused indicator when auto is on but not active', () => {
    render(
      <AutoModeToggle
        auto={true}
        isActive={false}
        intervalMs={3000}
        hasInputFocus={true}
        hasDirty={false}
        onToggle={vi.fn()}
      />
    );

    expect(screen.getByText(/paused \(editing\)/i)).toBeInTheDocument();
  });

  it('should show unsaved reason when paused due to dirty params', () => {
    render(
      <AutoModeToggle
        auto={true}
        isActive={false}
        intervalMs={3000}
        hasInputFocus={false}
        hasDirty={true}
        onToggle={vi.fn()}
      />
    );

    expect(screen.getByText(/paused \(unsaved\)/i)).toBeInTheDocument();
  });

  it('should not show paused indicator when active', () => {
    render(
      <AutoModeToggle
        auto={true}
        isActive={true}
        intervalMs={3000}
        hasInputFocus={false}
        hasDirty={false}
        onToggle={vi.fn()}
      />
    );

    expect(screen.queryByText(/paused/i)).not.toBeInTheDocument();
  });

  it('should call onToggle when checkbox is clicked', async () => {
    const user = userEvent.setup();
    const onToggle = vi.fn();

    render(
      <AutoModeToggle
        auto={false}
        isActive={false}
        intervalMs={3000}
        hasInputFocus={false}
        hasDirty={false}
        onToggle={onToggle}
      />
    );

    const checkbox = screen.getByRole('checkbox');
    await user.click(checkbox);

    expect(onToggle).toHaveBeenCalledWith(true);
  });

  it('should display interval in seconds', () => {
    render(
      <AutoModeToggle
        auto={true}
        isActive={true}
        intervalMs={5000}
        hasInputFocus={false}
        hasDirty={false}
        onToggle={vi.fn()}
      />
    );

    expect(screen.getByText(/auto \(5s\)/i)).toBeInTheDocument();
  });

  it('should prioritize editing reason over unsaved', () => {
    render(
      <AutoModeToggle
        auto={true}
        isActive={false}
        intervalMs={3000}
        hasInputFocus={true}
        hasDirty={true}
        onToggle={vi.fn()}
      />
    );

    expect(screen.getByText(/paused \(editing\)/i)).toBeInTheDocument();
    expect(screen.queryByText(/unsaved/i)).not.toBeInTheDocument();
  });

  it('should style label differently when paused', () => {
    const { rerender } = render(
      <AutoModeToggle
        auto={true}
        isActive={true}
        intervalMs={3000}
        hasInputFocus={false}
        hasDirty={false}
        onToggle={vi.fn()}
      />
    );

    const activeLabel = screen.getByText(/auto \(3s\)/i);
    expect(activeLabel.className).toContain('text-neutral-400');

    rerender(
      <AutoModeToggle
        auto={true}
        isActive={false}
        intervalMs={3000}
        hasInputFocus={true}
        hasDirty={false}
        onToggle={vi.fn()}
      />
    );

    const pausedLabel = screen.getByText(/auto \(3s\)/i);
    expect(pausedLabel.className).toContain('text-yellow-400');
  });
});
