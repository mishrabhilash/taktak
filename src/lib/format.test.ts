import { describe, expect, it } from 'vitest';
import {
  formatAudioDevice,
  formatMs,
  formatPercent,
  formatSampleRate,
  idleSleepChoices,
  idleSleepLabel,
  plural,
  unit,
} from './format';

describe('unit', () => {
  it('clamps to 0..1 and turns NaN into 0', () => {
    expect(unit(0.42)).toBe(0.42);
    expect(unit(-0.1)).toBe(0);
    expect(unit(1.5)).toBe(1);
    expect(unit(Number.NaN)).toBe(0);
    expect(unit(Number.POSITIVE_INFINITY)).toBe(0);
  });
});

describe('formatPercent', () => {
  it('shows whole percentages', () => {
    expect(formatPercent(0)).toBe('0%');
    expect(formatPercent(0.7)).toBe('70%');
    expect(formatPercent(0.25)).toBe('25%');
    expect(formatPercent(1)).toBe('100%');
  });

  it('rounds to the nearest percent', () => {
    expect(formatPercent(0.004)).toBe('0%');
    expect(formatPercent(0.005)).toBe('1%');
    expect(formatPercent(0.666)).toBe('67%');
    // Float noise from a 0.01-step range input.
    expect(formatPercent(0.1 + 0.2)).toBe('30%');
    expect(formatPercent(0.57)).toBe('57%');
  });

  it('never goes outside 0–100%', () => {
    expect(formatPercent(-1)).toBe('0%');
    expect(formatPercent(3)).toBe('100%');
    expect(formatPercent(Number.NaN)).toBe('0%');
  });
});

describe('formatMs', () => {
  it('shows one decimal below 100 ms and whole numbers above', () => {
    expect(formatMs(5.94)).toBe('5.9 ms');
    expect(formatMs(0.25)).toBe('0.3 ms');
    expect(formatMs(99.94)).toBe('99.9 ms');
    expect(formatMs(123.4)).toBe('123 ms');
    expect(formatMs(Number.NaN)).toBe('–');
  });
});

describe('formatSampleRate', () => {
  it('shows kHz', () => {
    expect(formatSampleRate(48000)).toBe('48 kHz');
    expect(formatSampleRate(44100)).toBe('44.1 kHz');
    expect(formatSampleRate(96000)).toBe('96 kHz');
  });
});

describe('formatAudioDevice', () => {
  it('joins what is known about the device', () => {
    expect(
      formatAudioDevice({
        device: 'MacBook Pro Speakers',
        sampleRate: 48000,
        bufferFrames: 64,
        state: 'ok',
        message: null,
      }),
    ).toBe('MacBook Pro Speakers · 48 kHz · 64-frame buffer');
    expect(
      formatAudioDevice({
        device: null,
        sampleRate: null,
        bufferFrames: null,
        state: 'fault',
        message: 'x',
      }),
    ).toBe('No output device');
  });
});

describe('plural', () => {
  it('picks the right form', () => {
    expect(plural(1, 'warning')).toBe('1 warning');
    expect(plural(0, 'warning')).toBe('0 warnings');
    expect(plural(3, 'warning')).toBe('3 warnings');
    expect(plural(2, 'key press', 'key presses')).toBe('2 key presses');
  });
});

describe('idle sleep', () => {
  it('names the idle time', () => {
    expect(idleSleepLabel(0)).toBe('Never');
    expect(idleSleepLabel(1)).toBe('After 1 minute');
    expect(idleSleepLabel(5)).toBe('After 5 minutes');
    expect(idleSleepLabel(60)).toBe('After 1 hour');
    expect(idleSleepLabel(120)).toBe('After 2 hours');
    expect(idleSleepLabel(90)).toBe('After 90 minutes');
  });

  it('offers the standard choices, and a hand-edited value in its place', () => {
    expect(idleSleepChoices(5)).toEqual([1, 2, 5, 10, 15, 30, 60, 0]);
    expect(idleSleepChoices(0)).toEqual([1, 2, 5, 10, 15, 30, 60, 0]);
    expect(idleSleepChoices(7)).toEqual([1, 2, 5, 7, 10, 15, 30, 60, 0]);
    expect(idleSleepChoices(240)).toEqual([1, 2, 5, 10, 15, 30, 60, 240, 0]);
  });
});
