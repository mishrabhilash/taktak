import { describe, expect, it, vi } from 'vitest';
import { copyText } from './clipboard';

describe('copyText', () => {
  it('writes the text through the Clipboard API', async () => {
    const writeText = vi.fn(async () => undefined);
    await expect(copyText('tccutil reset ListenEvent tech.taktak.app', { writeText })).resolves.toBe(
      true,
    );
    expect(writeText).toHaveBeenCalledWith('tccutil reset ListenEvent tech.taktak.app');
  });

  it('reports failure when nothing can copy (no DOM here)', async () => {
    const writeText = vi.fn(async () => {
      throw new Error('NotAllowedError');
    });
    await expect(copyText('x', { writeText })).resolves.toBe(false);
    await expect(copyText('x', undefined)).resolves.toBe(false);
  });
});
