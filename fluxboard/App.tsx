// Root application component

import { useEffect } from 'react';
import { Outlet } from 'react-router-dom';
import Nav from './Nav';
import type { PathProfile, UiSurfaceContract } from './config/uiProfiles';
import { useSuiteStore, type FluxboardSuite } from './stores';
import './lib/realtime/runtimeBridge';

const PROFILE_SUITE_MAP: Record<PathProfile, FluxboardSuite> = {
  default: 'all',
  tokenmm: 'dex_arb',
  equities: 'equities',
  lp: 'dex_arb',
};

function resolveSuiteForProfile(profile: PathProfile): FluxboardSuite {
  return PROFILE_SUITE_MAP[profile];
}

type AppProps = {
  profile: PathProfile;
  surface?: UiSurfaceContract;
};

export default function App({ profile, surface }: AppProps) {
  const setSuite = useSuiteStore((state) => state.setSuite);
  const suite = resolveSuiteForProfile(profile);

  useEffect(() => {
    setSuite(suite);
  }, [setSuite, suite]);

  return (
    <div className="flex flex-col h-screen overflow-hidden bg-bg-base text-text-secondary font-sans">
      {/* Skip to content for keyboard and screen readers */}
      <a
        href="#main"
        className="sr-only focus:not-sr-only focus:absolute focus:top-2 focus:left-2 focus:z-50 bg-bg-surface text-text-primary px-2 py-1 rounded-md text-sm"
      >
        Skip to content
      </a>
      <Nav profile={profile} surface={surface} />
      <main id="main" className="flex-1 overflow-hidden min-h-0">
        <Outlet />
      </main>
    </div>
  );
}
