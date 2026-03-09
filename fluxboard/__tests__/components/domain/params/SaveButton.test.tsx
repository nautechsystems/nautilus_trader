/**
 * Tests for SaveButton component
 */

import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { SaveButton } from '@/components/domain/params/SaveButton';

describe('SaveButton', () => {
  it('should render enabled when dirty and no errors', () => {
    render(
      <SaveButton
        strategyId="test-strategy"
        isDirty={true}
        isSaving={false}
        hasError={false}
        onSave={vi.fn()}
      />
    );

    const button = screen.getByRole('button', { name: /save/i });
    expect(button).not.toBeDisabled();
    expect(button).toHaveTextContent('Save');
  });

  it('should render disabled when not dirty', () => {
    render(
      <SaveButton
        strategyId="test-strategy"
        isDirty={false}
        isSaving={false}
        hasError={false}
        onSave={vi.fn()}
      />
    );

    const button = screen.getByRole('button', { name: /save/i });
    expect(button).toBeDisabled();
  });

  it('should render disabled when has errors', () => {
    render(
      <SaveButton
        strategyId="test-strategy"
        isDirty={true}
        isSaving={false}
        hasError={true}
        onSave={vi.fn()}
      />
    );

    const button = screen.getByRole('button', { name: /save/i });
    expect(button).toBeDisabled();
  });

  it('should show loading state when saving', () => {
    render(
      <SaveButton
        strategyId="test-strategy"
        isDirty={true}
        isSaving={true}
        hasError={false}
        onSave={vi.fn()}
      />
    );

    const button = screen.getByRole('button');
    expect(button).toHaveTextContent('...');
    expect(button).toBeDisabled();
  });

  it('should call onSave when clicked', async () => {
    const user = userEvent.setup();
    const onSave = vi.fn();

    render(
      <SaveButton
        strategyId="test-strategy"
        isDirty={true}
        isSaving={false}
        hasError={false}
        onSave={onSave}
      />
    );

    const button = screen.getByRole('button', { name: /save/i });
    await user.click(button);

    expect(onSave).toHaveBeenCalledTimes(1);
  });

  it('should not call onSave when disabled', async () => {
    const user = userEvent.setup();
    const onSave = vi.fn();

    render(
      <SaveButton
        strategyId="test-strategy"
        isDirty={false}
        isSaving={false}
        hasError={false}
        onSave={onSave}
      />
    );

    const button = screen.getByRole('button');
    await user.click(button);

    expect(onSave).not.toHaveBeenCalled();
  });

  it('should apply correct styles when enabled', () => {
    render(
      <SaveButton
        strategyId="test-strategy"
        isDirty={true}
        isSaving={false}
        hasError={false}
        onSave={vi.fn()}
      />
    );

    const button = screen.getByRole('button', { name: /save/i });
    expect(button.className).toContain('bg-emerald-600');
  });

  it('should apply correct styles when disabled', () => {
    render(
      <SaveButton
        strategyId="test-strategy"
        isDirty={false}
        isSaving={false}
        hasError={false}
        onSave={vi.fn()}
      />
    );

    const button = screen.getByRole('button');
    expect(button.className).toContain('cursor-not-allowed');
  });
});
