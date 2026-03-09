const DEFAULT_PULSE_BASE_PATH = "/pulse/";
const PULSE_SEGMENT = "/pulse";

export function normalizeBasePath(rawValue: string | undefined, fallback: string = DEFAULT_PULSE_BASE_PATH): string {
  const trimmed = (rawValue || "").trim();
  if (!trimmed) {
    return fallback;
  }

  const prefixed = trimmed.startsWith("/") ? trimmed : `/${trimmed}`;
  return prefixed.endsWith("/") ? prefixed : `${prefixed}/`;
}

function joinPath(basePath: string, path: string): string {
  const trimmedBase = basePath.endsWith("/") ? basePath.slice(0, -1) : basePath;
  const trimmedPath = path.replace(/^\/+/, "");
  if (!trimmedPath) {
    return trimmedBase ? `${trimmedBase}/` : "/";
  }
  return trimmedBase ? `${trimmedBase}/${trimmedPath}` : `/${trimmedPath}`;
}

export function getPulseBasePath(): string {
  return normalizeBasePath(import.meta.env.VITE_PULSE_UI_BASE_PATH, DEFAULT_PULSE_BASE_PATH);
}

export function getPulseShellBasePath(): string {
  const pulseBasePath = getPulseBasePath();
  const withoutTrailingSlash = pulseBasePath.endsWith("/")
    ? pulseBasePath.slice(0, -1)
    : pulseBasePath;

  if (withoutTrailingSlash.toLowerCase().endsWith(PULSE_SEGMENT)) {
    const shellBasePath = withoutTrailingSlash.slice(0, -PULSE_SEGMENT.length);
    return shellBasePath ? `${shellBasePath}/` : "/";
  }

  return pulseBasePath;
}

export function buildPulseHref(): string {
  return getPulseBasePath();
}

export function buildShellHref(path: string): string {
  return joinPath(getPulseShellBasePath(), path);
}
