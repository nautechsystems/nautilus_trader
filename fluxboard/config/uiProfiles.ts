import type { PanelId } from '../components/layout/PanelRegistry';

type NavLink = {
  path: string;
  label: string;
};

type ExternalLink = {
  port: number;
  label: string;
  pathSuffix?: string;
};

export type PathProfile = 'default' | 'tokenmm' | 'equities' | 'lp';
export type StableProfile = Exclude<PathProfile, 'default'>;

export type ProfileDefinition = {
  profile: StableProfile;
  aliases: readonly string[];
  basePath: `/${string}`;
};

export type UiSurfaceContract = {
  profile: PathProfile;
  homeRoutePath: string;
  navLinks: readonly NavLink[];
  externalLinks: readonly ExternalLink[];
  routePaths: readonly string[];
  allowedPanels: readonly PanelId[];
};

const TRADER_NAV_LINKS = [
  { path: '/', label: 'Dashboard' },
  { path: '/params', label: 'Params' },
  { path: '/signal', label: 'Signal' },
  { path: '/trades', label: 'Trades' },
  { path: '/pnl', label: 'PnL' },
  { path: '/balances', label: 'Balances' },
  { path: '/market-data', label: 'MD' },
  { path: '/fv', label: 'FV' },
  { path: '/fx', label: 'FX' },
  { path: '/alerts', label: 'Alerts' },
  { path: '/scanners', label: 'Scanners' },
] as const satisfies readonly NavLink[];

const TRADER_ROUTE_PATHS = [
  '/',
  '/dashboard',
  '/params',
  '/signal',
  '/trades',
  '/pnl',
  '/balances',
  '/market-data',
  '/fv',
  '/fx',
  '/alerts',
  '/scanners',
  '/scanners-harness',
] as const;

const MAKER_SUITE_CORE_NAV_LINKS = [
  { path: '/', label: 'Dashboard' },
  { path: '/signal', label: 'Signal' },
  { path: '/params', label: 'Params' },
  { path: '/balances', label: 'Balances' },
  { path: '/trades', label: 'Trades' },
  { path: '/alerts', label: 'Alerts' },
] as const satisfies readonly NavLink[];

const MAKER_CORE_ROUTE_PATHS = [
  '/',
  '/dashboard',
  '/params',
  '/signal',
  '/trades',
  '/balances',
  '/alerts',
] as const;

const TOKENMM_NAV_LINKS = MAKER_SUITE_CORE_NAV_LINKS;

const TOKENMM_ROUTE_PATHS = MAKER_CORE_ROUTE_PATHS;

const PANEL_IDS = [
  'params',
  'trades',
  'signal',
  'fv',
  'balances',
  'alerts',
] as const satisfies readonly PanelId[];
const MAKER_SUITE_CORE_PANEL_IDS = [
  'signal',
  'params',
  'balances',
  'trades',
  'alerts',
] as const satisfies readonly PanelId[];

const MAKER_CORE_SURFACE_PROPS = {
  navLinks: MAKER_SUITE_CORE_NAV_LINKS,
  externalLinks: [] as const,
  routePaths: MAKER_CORE_ROUTE_PATHS,
  allowedPanels: MAKER_SUITE_CORE_PANEL_IDS,
} as const;

const TOKENMM_SURFACE_PROPS = {
  homeRoutePath: '/dashboard',
  navLinks: TOKENMM_NAV_LINKS,
  externalLinks: [] as const,
  routePaths: TOKENMM_ROUTE_PATHS,
  allowedPanels: MAKER_SUITE_CORE_PANEL_IDS,
} as const;

const LP_SURFACE_PROPS = {
  homeRoutePath: '/hedger',
  navLinks: [{ path: '/', label: 'Hedger' }] as const,
  externalLinks: [] as const,
  routePaths: ['/', '/hedger'] as const,
  allowedPanels: [] as const,
} as const;

const PROFILE_DEFINITIONS: Record<StableProfile, ProfileDefinition> = {
  tokenmm: {
    profile: 'tokenmm',
    aliases: ['tokenmm', 'tokenm'],
    basePath: '/tokenmm',
  },
  equities: {
    profile: 'equities',
    aliases: ['equities'],
    basePath: '/equities',
  },
  lp: {
    profile: 'lp',
    aliases: ['lp'],
    basePath: '/lp',
  },
} as const;

const SURFACES: Record<PathProfile, UiSurfaceContract> = {
  default: {
    profile: 'default',
    homeRoutePath: '/dashboard',
    navLinks: TRADER_NAV_LINKS,
    externalLinks: [
      { port: 8090, label: 'Pulse' },
      { port: 8092, label: 'Nexus', pathSuffix: '/catalog/' },
    ],
    routePaths: TRADER_ROUTE_PATHS,
    allowedPanels: PANEL_IDS,
  },
  tokenmm: {
    profile: 'tokenmm',
    ...TOKENMM_SURFACE_PROPS,
  },
  equities: {
    profile: 'equities',
    homeRoutePath: '/dashboard',
    ...MAKER_CORE_SURFACE_PROPS,
  },
  lp: {
    profile: 'lp',
    ...LP_SURFACE_PROPS,
  },
};

export function resolvePathProfile(value: string | null | undefined): PathProfile {
  const raw = String(value || '')
    .trim()
    .toLowerCase();

  if (!raw) {
    return 'default';
  }

  for (const definition of Object.values(PROFILE_DEFINITIONS)) {
    if (definition.aliases.includes(raw)) {
      return definition.profile;
    }
  }

  return 'default';
}

export function resolvePathnameProfile(pathname: string | null | undefined): PathProfile {
  const firstSegment = String(pathname || '')
    .split('/')
    .filter(Boolean)[0];
  return resolvePathProfile(firstSegment);
}

export function buildProfilePath(profile: PathProfile, routePath: string): string {
  const normalizedPath = routePath.startsWith('/') ? routePath : `/${routePath}`;
  if (profile === 'default') {
    return normalizedPath;
  }
  const definition = PROFILE_DEFINITIONS[profile];
  if (normalizedPath === '/') {
    return definition.basePath;
  }
  return `${definition.basePath}${normalizedPath}`;
}

export function getUiSurface(profile: PathProfile): UiSurfaceContract {
  return SURFACES[profile];
}

export function getProfileDefinition(profile: StableProfile): ProfileDefinition {
  return PROFILE_DEFINITIONS[profile];
}
