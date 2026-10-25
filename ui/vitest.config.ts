import { defineConfig } from "vitest/config";

// Unit tests (format engine and other pure logic). End-to-end tests live in e2e/ and
// run under Playwright.
export default defineConfig({
  test: { include: ["src/**/*.test.ts"] },
});
