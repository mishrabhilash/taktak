import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { throttle } from './throttle';

describe('throttle', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('sends the first value at once', () => {
    const send = vi.fn();
    const t = throttle(send, 33);
    t.call(1);
    expect(send).toHaveBeenCalledExactlyOnceWith(1);
  });

  it('holds later values until the interval passes, then sends only the newest', () => {
    const send = vi.fn();
    const t = throttle(send, 33);
    t.call(1);
    vi.advanceTimersByTime(10);
    t.call(2);
    t.call(3);
    expect(send).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(23);
    expect(send).toHaveBeenCalledTimes(2);
    expect(send).toHaveBeenLastCalledWith(3);
    vi.advanceTimersByTime(1000);
    expect(send).toHaveBeenCalledTimes(2);
  });

  it('sends at once again after a quiet interval', () => {
    const send = vi.fn();
    const t = throttle(send, 33);
    t.call(1);
    vi.advanceTimersByTime(50);
    t.call(2);
    expect(send).toHaveBeenCalledTimes(2);
    expect(send).toHaveBeenLastCalledWith(2);
  });

  it('flush sends the queued value now and nothing later', () => {
    const send = vi.fn();
    const t = throttle(send, 33);
    t.call(1);
    t.call(2);
    t.flush();
    expect(send.mock.calls).toEqual([[1], [2]]);
    vi.advanceTimersByTime(100);
    expect(send).toHaveBeenCalledTimes(2);
  });

  it('flush with nothing queued sends nothing', () => {
    const send = vi.fn();
    const t = throttle(send, 33);
    t.call(1);
    t.flush();
    t.flush();
    expect(send).toHaveBeenCalledTimes(1);
  });

  it('cancel drops the queued value', () => {
    const send = vi.fn();
    const t = throttle(send, 33);
    t.call(1);
    t.call(2);
    t.cancel();
    vi.advanceTimersByTime(100);
    expect(send.mock.calls).toEqual([[1]]);
  });

  it('keeps a fast drag to about 30 sends a second and always sends the last value', () => {
    const send = vi.fn();
    const t = throttle(send, 33);
    // A one-second drag reporting a new position every 4 ms (250 Hz pointer).
    let value = 0;
    for (let ms = 0; ms < 1000; ms += 4) {
      value = ms / 1000;
      t.call(value);
      vi.advanceTimersByTime(4);
    }
    t.flush();
    expect(send.mock.calls.length).toBeGreaterThanOrEqual(25);
    expect(send.mock.calls.length).toBeLessThanOrEqual(32);
    expect(send).toHaveBeenLastCalledWith(value);
  });
});
