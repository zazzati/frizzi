import { defineConfig, mergeConfig } from 'vitest/config';
import viteConfig from './vite.config.ts';

export default mergeConfig(viteConfig, defineConfig({
 test: {
  environment: 'node',
  include: ['src/**/*.test.ts', 'tests/unit/**/*.test.ts'],
  coverage: {
   provider: 'v8',
   include: ['src/lib/**/*.ts'],
   exclude: ['src/lib/**/*.test.ts'],
   reporter: ['text', 'lcov'],
   reportsDirectory: 'coverage'
  }
 }
}));
