import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    include: ["tests/**/*.test.ts"],
    fileParallelism: false,
    hookTimeout: 300_000,
    testTimeout: 10_000,
  },
});
