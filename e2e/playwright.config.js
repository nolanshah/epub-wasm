// @ts-check
import { defineConfig } from '@playwright/test';

// Serves a directory assembled by setup-serve.sh: the client-test demo pages,
// the wasm pkg, and the fixture EPUB, all same-origin.
export default defineConfig({
  testDir: './tests',
  timeout: 30_000,
  retries: 0,
  reporter: [['list']],
  use: {
    baseURL: 'http://127.0.0.1:8199',
    // Use the installed Chrome locally (no browser download); CI installs
    // chromium and unsets PW_CHANNEL.
    channel: process.env.CI ? undefined : 'chrome',
    headless: true,
  },
  webServer: {
    command: 'sh setup-serve.sh && python3 -m http.server 8199 -d .serve',
    url: 'http://127.0.0.1:8199/index.html',
    reuseExistingServer: false,
    timeout: 20_000,
  },
});
