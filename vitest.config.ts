import { defineConfig, mergeConfig } from 'vitest/config';
import viteConfig from './vite.config.ts';

// Unit tests for the UI's pure helpers and the mock backend. They run in Node: no browser,
// no DOM, nothing downloaded.
export default mergeConfig(
  viteConfig,
  defineConfig({
    test: {
      include: ['src/**/*.test.ts'],
      environment: 'node',
      restoreMocks: true,
      unstubGlobals: true,
    },
  }),
);
