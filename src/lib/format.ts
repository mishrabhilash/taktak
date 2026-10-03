// Small display formatters shared by the views.

import type { AudioStatus } from './types';

/** Clamps to 0..1; NaN becomes 0. */
export function unit(value: number): number {
  return Number.isFinite(value) ? Math.min(1, Math.max(0, value)) : 0;
}

/** A 0..1 level as a whole percentage: 0.7 → "70%". */
export function formatPercent(value: number): string {
  return `${Math.round(unit(value) * 100)}%`;
}

/** Milliseconds with one decimal below 100 ms: 5.94 → "5.9 ms", 123.4 → "123 ms". */
export function formatMs(ms: number): string {
  if (!Number.isFinite(ms)) return '–';
  return ms < 100 ? `${ms.toFixed(1)} ms` : `${Math.round(ms)} ms`;
}

/** 48000 → "48 kHz", 44100 → "44.1 kHz". */
export function formatSampleRate(hz: number): string {
  const khz = hz / 1000;
  return `${Number.isInteger(khz) ? khz : khz.toFixed(1)} kHz`;
}

/** The output device line: "MacBook Pro Speakers · 48 kHz · 64-frame buffer". */
export function formatAudioDevice(audio: AudioStatus): string {
  const parts = [audio.device ?? 'No output device'];
  if (audio.sampleRate) parts.push(formatSampleRate(audio.sampleRate));
  if (audio.bufferFrames) parts.push(`${audio.bufferFrames}-frame buffer`);
  return parts.join(' · ');
}

/** "1 warning", "3 warnings". */
export function plural(count: number, one: string, many = `${one}s`): string {
  return `${count} ${count === 1 ? one : many}`;
}
