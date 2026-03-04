import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';

export type ViewportTier = 'phone' | 'tablet' | 'desktop';
export type DensityMode = 'mobile' | 'desktop';

export interface MobileLayoutState {
  viewport: ViewportTier;
  isMobile: boolean;
  isMobileViewport: boolean;
  density: DensityMode;
  isTouch: boolean;
  width: number;
  height: number;
}

const DEFAULT_STATE: MobileLayoutState = {
  viewport: 'desktop',
  isMobile: false,
  isMobileViewport: false,
  density: 'desktop',
  isTouch: false,
  width: 1280,
  height: 720,
};

const PHONE_MAX = 639;
const TABLET_MAX = 1023;

function evaluateViewport(width: number): ViewportTier {
  if (width <= PHONE_MAX) {
    return 'phone';
  }
  if (width <= TABLET_MAX) {
    return 'tablet';
  }
  return 'desktop';
}

function detectLayout(): MobileLayoutState {
  if (typeof window === 'undefined') {
    return DEFAULT_STATE;
  }
  const width = Math.max(window.innerWidth || DEFAULT_STATE.width, 0);
  const height = Math.max(window.innerHeight || DEFAULT_STATE.height, 0);
  const viewport = evaluateViewport(width);
  const isTouch = typeof navigator !== 'undefined' && navigator.maxTouchPoints > 0;
  const isMobileViewport = viewport !== 'desktop';
  const prefersMobileUI = isTouch && isMobileViewport;
  const density: DensityMode = prefersMobileUI ? 'mobile' : 'desktop';
  return { viewport, isMobile: prefersMobileUI, isMobileViewport, density, isTouch, width, height };
}

const MobileLayoutContext = createContext<MobileLayoutState>(DEFAULT_STATE);

export function MobileLayoutProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<MobileLayoutState>(() => detectLayout());

  useEffect(() => {
    if (typeof window === 'undefined') {
      return;
    }

    let animationFrame: number | null = null;

    const handleResize = () => {
      if (animationFrame !== null) {
        cancelAnimationFrame(animationFrame);
      }
      animationFrame = requestAnimationFrame(() => {
        animationFrame = null;
        setState(detectLayout());
      });
    };

    handleResize();
    window.addEventListener('resize', handleResize);
    window.addEventListener('orientationchange', handleResize);

    return () => {
      if (animationFrame !== null) {
        cancelAnimationFrame(animationFrame);
      }
      window.removeEventListener('resize', handleResize);
      window.removeEventListener('orientationchange', handleResize);
    };
  }, []);

  return (
    <MobileLayoutContext.Provider value={state}>{children}</MobileLayoutContext.Provider>
  );
}

export function useMobileLayout() {
  return useContext(MobileLayoutContext);
}

export function useDensityMode(override?: DensityMode) {
  const layout = useMobileLayout();
  return override ?? layout.density;
}
