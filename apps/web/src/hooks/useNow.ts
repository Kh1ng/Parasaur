import { useEffect, useReducer } from 'react';

/** Re-evaluate time on every render, including new data arriving between
 * ticks. A stored timestamp can predate a freshly received observation. */
export function useNow(): number {
  const [, tick] = useReducer(value => value + 1, 0);
  useEffect(() => {
    const timer = window.setInterval(tick, 30_000);
    return () => window.clearInterval(timer);
  }, []);
  return Date.now();
}
