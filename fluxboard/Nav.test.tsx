// Navigation tests

import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import Nav from './Nav';
import { getUiSurface } from './config/uiProfiles';

function renderAt(path: string, profile: 'default' | 'tokenmm' | 'equities' = 'default') {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <Nav profile={profile} surface={getUiSurface(profile)} />
    </MemoryRouter>
  );
}

describe('Nav', () => {
  it('renders primary navigation with all links', () => {
    renderAt('/');

    const expected = [
      'Dashboard',
      'Params',
      'Signal',
      'Trades',
      'PnL',
      'Balances',
      'FV',
      'FX',
      'Alerts',
      'Scanners',
      'Hedger',
    ];

    for (const label of expected) {
      expect(screen.getByRole('link', { name: label })).toBeInTheDocument();
    }

    // Has proper navigation landmarks
    const nav = screen.getByRole('navigation', { name: 'Primary' });
    expect(nav).toBeInTheDocument();
  });

  it('marks current route active with aria-current and active styles', () => {
    renderAt('/alerts');
    const alerts = screen.getByRole('link', { name: 'Alerts' });
    expect(alerts).toHaveAttribute('aria-current', 'page');
    expect(alerts).toHaveClass('nav-link--active');
  });

  it('considers nested routes active (prefix match)', () => {
    renderAt('/trades/details/abc');
    const trades = screen.getByRole('link', { name: 'Trades' });
    expect(trades).toHaveAttribute('aria-current', 'page');
  });

  it('is horizontally scrollable on small screens', () => {
    renderAt('/');
    const container = screen.getByTestId('primary-nav-links');
    expect(container.className).toMatch(/overflow-x-auto/);
  });

  it('styles external links separately from internal navigation links', () => {
    renderAt('/');

    const pulseLink = screen.getByRole('link', { name: /pulse ↗/i });
    expect(pulseLink).toHaveClass('nav-link--external');
    expect(pulseLink).toHaveAttribute('target', '_blank');
    expect(pulseLink).toHaveAttribute('rel', 'noopener noreferrer');
  });

  it('shows alerts and hides order-view/external links for tokenmm', () => {
    renderAt('/tokenmm', 'tokenmm');

    const visible = ['Dashboard', 'Signal', 'Params', 'Balances', 'Trades', 'Alerts'];
    for (const label of visible) {
      expect(screen.getByRole('link', { name: label })).toBeInTheDocument();
    }

    const hidden = ['Equities', 'PnL', 'MD', 'FX', 'Scanners', 'Hedger', 'Orders'];
    for (const label of hidden) {
      expect(screen.queryByRole('link', { name: label })).not.toBeInTheDocument();
    }

    expect(screen.queryByRole('link', { name: /pulse ↗/i })).not.toBeInTheDocument();
    expect(screen.queryByRole('link', { name: /nexus ↗/i })).not.toBeInTheDocument();
  });

  it('prefixes tokenmm nav links with /tokenmm', () => {
    renderAt('/tokenmm/signal', 'tokenmm');

    const dashboard = screen.getByRole('link', { name: 'Dashboard' });
    const signal = screen.getByRole('link', { name: 'Signal' });
    const params = screen.getByRole('link', { name: 'Params' });

    expect(dashboard).toHaveAttribute('href', '/tokenmm');
    expect(signal).toHaveAttribute('href', '/tokenmm/signal');
    expect(params).toHaveAttribute('href', '/tokenmm/params');
    expect(signal).toHaveAttribute('aria-current', 'page');
    expect(signal).toHaveClass('nav-link--active');
    expect(dashboard).not.toHaveAttribute('aria-current');
    expect(dashboard).not.toHaveClass('nav-link--active');
  });

  it('prefixes equities nav links with /equities', () => {
    renderAt('/equities/alerts', 'equities');

    const dashboard = screen.getByRole('link', { name: 'Dashboard' });
    const alerts = screen.getByRole('link', { name: 'Alerts' });

    expect(dashboard).toHaveAttribute('href', '/equities');
    expect(alerts).toHaveAttribute('href', '/equities/alerts');
    expect(alerts).toHaveAttribute('aria-current', 'page');
    expect(alerts).toHaveClass('nav-link--active');
    expect(dashboard).not.toHaveAttribute('aria-current');
    expect(dashboard).not.toHaveClass('nav-link--active');
  });
});
