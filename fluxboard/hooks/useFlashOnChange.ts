/**
 * Custom hook for flash animations on value changes
 * Extracts duplicated logic from BalanceGroup and BalanceRow
 */

import { useState, useEffect, useRef } from 'react';

type FlashDirection = 'increase' | 'decrease' | null;

export function useFlashOnChange<T extends number>(value: T, duration = 500) {
  const [flash, setFlash] = useState<FlashDirection>(null);
  const prevValue = useRef(value);

  useEffect(() => {
    if (value !== prevValue.current) {
      const previous = prevValue.current;
      const direction: FlashDirection = value > previous ? 'increase' : 'decrease';
      setFlash(direction);

      const timer = setTimeout(() => setFlash(null), duration);
      prevValue.current = value;

      return () => clearTimeout(timer);
    }
  }, [value, duration]);

  return { flash, prevValue: prevValue.current };
}
