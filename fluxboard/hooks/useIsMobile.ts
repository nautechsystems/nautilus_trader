import { useEffect, useState } from 'react';

const DEFAULT_BREAKPOINT = 768;

function getMatches(breakpoint: number): boolean {
  if (typeof window === 'undefined') {
    return false;
  }

  const query = `(max-width: ${breakpoint - 1}px)`;
  if (typeof window.matchMedia === 'function') {
    return window.matchMedia(query).matches;
  }

  return (window.innerWidth || 0) < breakpoint;
}

export function useIsMobile(breakpoint: number = DEFAULT_BREAKPOINT): boolean {
  const [isMobile, setIsMobile] = useState<boolean>(() => getMatches(breakpoint));

  useEffect(() => {
    if (typeof window === 'undefined') return;

    const handleChange = () => setIsMobile(getMatches(breakpoint));

    const mql = typeof window.matchMedia === 'function'
      ? window.matchMedia(`(max-width: ${breakpoint - 1}px)`)
      : null;

    mql?.addEventListener('change', handleChange);
    window.addEventListener('resize', handleChange);

    // Ensure state is in sync immediately
    handleChange();

    return () => {
      mql?.removeEventListener('change', handleChange);
      window.removeEventListener('resize', handleChange);
    };
  }, [breakpoint]);

  return isMobile;
}
