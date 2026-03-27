// Application entry point

import React, { lazy, Suspense } from 'react';
import { createRoot } from 'react-dom/client';
import {
  Navigate,
  createBrowserRouter,
  RouterProvider,
  useLocation,
  useParams,
  type RouteObject,
} from 'react-router-dom';
import { Toaster } from 'sonner';
import './styles.css';

import App from './App';
import { MobileLayoutProvider } from './hooks/useMobileLayout';
const Params = lazy(() => import('./Params'));
const Signal = lazy(() => import('./Signal'));
const Trades = lazy(() => import('./Trades'));
const FV = lazy(() => import('./FV'));
const Fx = lazy(() => import('./Fx'));
const Hedger = lazy(() => import('./Hedger'));
const Balances = lazy(() => import('./Balances'));
const Alerts = lazy(() => import('./Alerts'));
const PnL = lazy(() => import('./PnL'));
const Scanner = lazy(() => import('./Scanner'));
const MarketData = lazy(() => import('./MarketData'));
const ScannersHarness = import.meta.env?.DEV ? lazy(() => import('./pages/ScannersHarness')) : null;
import { DashboardLayout } from './components/layout/DashboardLayout';
import Title from './Title';
import { AppErrorBoundary } from './components/ErrorBoundary';
import { getUiSurface, type PathProfile, type UiSurfaceContract } from './config/uiProfiles';

// Build ID check disabled - bundle hash in filename is sufficient for cache busting
// (Previous implementation caused infinite reloads due to dynamic timestamp fallback)

type RouteBuilderOptions = {
  includeScannersHarness: boolean;
  fallbackPath: string;
};

function pathToChildPath(path: string): string {
  return path.replace(/^\//, '');
}

export function buildFluxboardChildRoutes(
  surface: UiSurfaceContract,
  options: RouteBuilderOptions
): RouteObject[] {
  const routeElements: Record<string, RouteObject['element']> = {
    '/dashboard': (
      <Title title="Dashboard">
        <DashboardLayout
          preset="default"
          allowedPanels={surface.allowedPanels}
          storageScope={surface.profile}
        />
      </Title>
    ),
    '/params': (
      <Title title="Params">
        <Suspense fallback={<div />}>
          <Params />
        </Suspense>
      </Title>
    ),
    '/signal': (
      <Title title="Signal">
        <Suspense fallback={<div />}>
          <Signal />
        </Suspense>
      </Title>
    ),
    '/trades': (
      <Title title="Trades">
        <Suspense fallback={<div />}>
          <Trades />
        </Suspense>
      </Title>
    ),
    '/fv': (
      <Title title="Fair Value">
        <Suspense fallback={<div />}>
          <FV />
        </Suspense>
      </Title>
    ),
    '/pnl': (
      <Title title="PnL Report">
        <Suspense fallback={<div />}>
          <PnL />
        </Suspense>
      </Title>
    ),
    '/balances': (
      <Title title="Balances">
        <Suspense fallback={<div />}>
          <Balances />
        </Suspense>
      </Title>
    ),
    '/market-data': (
      <Title title="Market Data">
        <Suspense fallback={<div />}>
          <MarketData />
        </Suspense>
      </Title>
    ),
    '/fx': (
      <Title title="FX">
        <Suspense fallback={<div />}>
          <Fx />
        </Suspense>
      </Title>
    ),
    '/alerts': (
      <Title title="Alerts">
        <Suspense fallback={<div />}>
          <Alerts />
        </Suspense>
      </Title>
    ),
    '/hedger': (
      <Title title="LP Hedger">
        <Suspense fallback={<div />}>
          <Hedger />
        </Suspense>
      </Title>
    ),
    '/scanners': (
      <Title title="Scanners">
        <Suspense fallback={<div />}>
          <Scanner />
        </Suspense>
      </Title>
    ),
  };

  const pathSet = new Set(surface.routePaths);
  const homeElement = routeElements[surface.homeRoutePath];
  if (!homeElement) {
    throw new Error(`Missing home route element for ${surface.profile}: ${surface.homeRoutePath}`);
  }
  const routes: RouteObject[] = [
    {
      index: true,
      element: homeElement,
    },
  ];

  for (const [path, element] of Object.entries(routeElements)) {
    if (!pathSet.has(path)) {
      continue;
    }
    routes.push({
      path: pathToChildPath(path),
      element,
    });
  }

  if (options.includeScannersHarness && pathSet.has('/scanners-harness') && ScannersHarness) {
    routes.push({
      path: 'scanners-harness',
      element: (
        <Title title="Scanners Harness">
          <Suspense fallback={<div />}>
            <ScannersHarness />
          </Suspense>
        </Title>
      ),
    });
  }

  routes.push({ path: '*', element: <Navigate to={options.fallbackPath} replace /> });
  return routes;
}

export function buildTokenmmAliasTarget(
  splatPath: string | undefined,
  search: string = '',
  hash: string = ''
): string {
  const normalizedSplat = (splatPath || '').replace(/^\/+/, '');
  const basePath = normalizedSplat ? `/tokenmm/${normalizedSplat}` : '/tokenmm';
  return `${basePath}${search}${hash}`;
}

function RedirectTokenmWithSplat() {
  const params = useParams();
  const location = useLocation();
  const target = buildTokenmmAliasTarget(params['*'], location.search, location.hash);
  return <Navigate to={target} replace />;
}

function createProfileRoute(
  profile: PathProfile,
  path: string,
): RouteObject {
  const surface = getUiSurface(profile);
  return {
    path,
    element: <App profile={profile} surface={surface} />,
    children: buildFluxboardChildRoutes(surface, {
      includeScannersHarness: Boolean(ScannersHarness),
      fallbackPath: path,
    }),
  };
}

export function buildFluxboardTopLevelRoutes(): RouteObject[] {
  return [
    createProfileRoute('default', '/'),
    {
      path: '/tokenm',
      element: <RedirectTokenmWithSplat />,
    },
    {
      path: '/tokenm/*',
      element: <RedirectTokenmWithSplat />,
    },
    createProfileRoute('tokenmm', '/tokenmm'),
    createProfileRoute('equities', '/equities'),
    createProfileRoute('lp', '/lp'),
    {
      path: '*',
      element: <Navigate to="/" replace />,
    },
  ];
}

const router = createBrowserRouter(buildFluxboardTopLevelRoutes());

createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <AppErrorBoundary>
      <MobileLayoutProvider>
        <RouterProvider router={router} />
        <Toaster position="top-right" theme="dark" />
      </MobileLayoutProvider>
    </AppErrorBoundary>
  </React.StrictMode>
);
