// Rate-limits slider updates: the first value goes out at once, then at most one per interval,
// and the latest value is always sent in the end (trailing edge, or at once with `flush`).

export interface Throttled<T> {
  /** Sends now if the interval has passed, otherwise queues `value` (replacing older ones). */
  call(value: T): void;
  /** Sends the queued value now, if any (e.g. when the slider is released). */
  flush(): void;
  /** Drops the queued value. */
  cancel(): void;
}

export function throttle<T>(send: (value: T) => void, intervalMs: number): Throttled<T> {
  let last = -Infinity;
  let queued: { value: T } | null = null;
  let timer: ReturnType<typeof setTimeout> | null = null;

  function fire(): void {
    if (timer !== null) clearTimeout(timer);
    timer = null;
    if (!queued) return;
    const { value } = queued;
    queued = null;
    last = Date.now();
    send(value);
  }

  return {
    call(value) {
      queued = { value };
      const wait = last + intervalMs - Date.now();
      if (wait <= 0) fire();
      else timer ??= setTimeout(fire, wait);
    },
    flush: fire,
    cancel() {
      if (timer !== null) clearTimeout(timer);
      timer = null;
      queued = null;
    },
  };
}
