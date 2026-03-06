import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { StatusPill } from './StatusPill';

describe('StatusPill', () => {
  it('renders label and data-status for semantic status', () => {
    render(<StatusPill status="ok" label="Live" />);
    const pill = screen.getByRole('status');
    expect(pill).toHaveAttribute('data-status', 'ok');
    expect(screen.getByText('Live')).toBeInTheDocument();
  });

  it('falls back to legacy variant mapping', () => {
    render(<StatusPill variant="pending" />);
    const pill = screen.getByRole('status');
    expect(pill).toHaveAttribute('data-status', 'warning');
    expect(screen.getByText('Pending')).toBeInTheDocument();
  });

  it('renders subLabel when provided', () => {
    render(<StatusPill status="info" label="RUNNER" subLabel="ON" layout="inline" />);
    expect(screen.getByText('RUNNER')).toBeInTheDocument();
    expect(screen.getByText('ON')).toBeInTheDocument();
  });
});
