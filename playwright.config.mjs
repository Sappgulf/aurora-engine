import { defineConfig } from "@playwright/test";

const chromeExecutable = process.platform === "darwin"
  ? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
  : undefined;

export default defineConfig({
  testDir: "./playtests/browser",
  outputDir: "./test-results/playwright",
  retries: process.env.CI ? 1 : 0,
  workers: 1,
  timeout: 120_000,
  reporter: [["line"]],
  use: {
    baseURL: "http://127.0.0.1:8080",
    browserName: "chromium",
    headless: true,
    launchOptions: {
      executablePath: chromeExecutable,
      args: ["--enable-unsafe-webgpu", "--use-angle=swiftshader"],
    },
    viewport: { width: 1280, height: 720 },
    screenshot: "only-on-failure",
    trace: "retain-on-failure",
  },
  projects: [
    { name: "chromium-dpr-1", use: { deviceScaleFactor: 1 } },
    { name: "chromium-dpr-2", use: { deviceScaleFactor: 2 } },
  ],
  webServer: {
    command: "python3 -m http.server 8080 --bind 127.0.0.1 --directory games/last-light/dist",
    url: "http://127.0.0.1:8080",
    reuseExistingServer: !process.env.CI,
    timeout: 30_000,
  },
});
