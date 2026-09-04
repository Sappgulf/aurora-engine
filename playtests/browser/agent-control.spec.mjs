import { test, expect } from "@playwright/test";
import { mkdir } from "node:fs/promises";

const screenshotRoot = "playtests/screenshots";
const platformerURL = "http://127.0.0.1:8082/";

// The platformer web build exposes a small JS bridge for external agents:
// window.auroraState() returns a JSON state snapshot string and
// window.auroraInjectKey(key, down) synthesizes engine input. Every bridge
// touch is guarded so a build without the bridge fails with a readable
// message instead of a generic "undefined is not a function" error.
async function readState(page) {
  return page.evaluate(() => {
    if (typeof window.auroraState !== "function") {
      throw new Error("window.auroraState is undefined: the platformer agent bridge is not registered in this web build");
    }
    try {
      return JSON.parse(window.auroraState());
    } catch (error) {
      throw new Error(`window.auroraState() did not return parseable JSON: ${error}`);
    }
  });
}

async function injectKey(page, key, down) {
  await page.evaluate(({ key, down }) => {
    if (typeof window.auroraInjectKey !== "function") {
      throw new Error("window.auroraInjectKey is undefined: the platformer agent bridge is not registered in this web build");
    }
    window.auroraInjectKey(key, down);
  }, { key, down });
}

test.beforeAll(async () => {
  await mkdir(screenshotRoot, { recursive: true });
});

test("agent can start the game and move the player via the JS bridge", async ({ page }) => {
  await page.goto(platformerURL);
  const canvas = page.locator("#aurora-canvas");
  await expect(canvas).toBeVisible();
  // SwiftShader + WASM boot takes several seconds; wait for the first
  // published state rather than a fixed sleep.
  await expect.poll(async () => {
    const state = await readState(page);
    return state && typeof state.screen === "string" ? "ready" : "booting";
  }, { timeout: 30_000, intervals: [500] }).toBe("ready");

  const initial = await readState(page);
  if (initial.screen !== "playing") {
    await injectKey(page, "Space", true);
    await injectKey(page, "Space", false);
    await expect.poll(async () => (await readState(page)).screen, {
      timeout: 10_000,
      intervals: [200],
    }).toBe("playing");
  }

  const start = await readState(page);
  await injectKey(page, "KeyD", true);
  await expect.poll(async () => {
    const state = await readState(page);
    return state.position[0] - start.position[0];
  }, { timeout: 15_000, intervals: [200] }).toBeGreaterThanOrEqual(60);
  await injectKey(page, "KeyD", false);

  const released = await readState(page);
  expect(released.position[0] - start.position[0]).toBeGreaterThanOrEqual(60);
  expect(released.screen).toBe("playing");

  await page.screenshot({ path: `${screenshotRoot}/agent-control-web.png` });

  // The bridge must keep publishing a stable, parseable snapshot carrying the
  // deterministic state hash promised by the agent control plane contract.
  const raw = await page.evaluate(() => {
    if (typeof window.auroraState !== "function") {
      throw new Error("window.auroraState is undefined: the platformer agent bridge is not registered in this web build");
    }
    return window.auroraState();
  });
  const stable = JSON.parse(raw);
  expect(stable).toHaveProperty("hash");
  expect(stable.screen).toBe("playing");
  expect(Array.isArray(stable.position)).toBe(true);
});

test("agent can pause, resume, and dash via the JS bridge", async ({ page }) => {
  await page.goto(platformerURL);
  const canvas = page.locator("#aurora-canvas");
  await expect(canvas).toBeVisible();
  await expect.poll(async () => {
    const state = await readState(page);
    return state && typeof state.screen === "string" ? "ready" : "booting";
  }, { timeout: 30_000, intervals: [500] }).toBe("ready");

  // Start the level.
  await injectKey(page, "Space", true);
  await injectKey(page, "Space", false);
  await expect.poll(async () => (await readState(page)).screen, {
    timeout: 10_000,
    intervals: [200],
  }).toBe("playing");

  // Pause through the bridge, verify the sim freezes, resume.
  await injectKey(page, "KeyP", true);
  await injectKey(page, "KeyP", false);
  await expect.poll(async () => (await readState(page)).screen, {
    timeout: 5_000,
    intervals: [200],
  }).toBe("paused");
  const pausedTicks = (await readState(page)).ticks;
  await page.waitForTimeout(600);
  expect((await readState(page)).ticks).toBe(pausedTicks);

  await injectKey(page, "KeyP", true);
  await injectKey(page, "KeyP", false);
  await expect.poll(async () => (await readState(page)).screen, {
    timeout: 5_000,
    intervals: [200],
  }).toBe("playing");

  // Dash: the burst must carry the player visibly rightward.
  const before = await readState(page);
  await injectKey(page, "ShiftLeft", true);
  await injectKey(page, "ShiftLeft", false);
  await expect.poll(async () => (await readState(page)).position[0], {
    timeout: 5_000,
    intervals: [200],
  }).toBeGreaterThan(before.position[0] + 40);
  const state = await readState(page);
  expect(state.screen).toBe("playing");
  expect(state).toHaveProperty("hash");
});

test("agent can load a level and verify the HUD counter", async ({ page }) => {
  await page.goto(platformerURL);
  const canvas = page.locator("#aurora-canvas");
  await expect(canvas).toBeVisible();
  // SwiftShader + WASM boot takes several seconds; wait for the first
  // published state rather than a fixed sleep.
  await expect.poll(async () => {
    const state = await readState(page);
    return state && typeof state.screen === "string" ? "ready" : "booting";
  }, { timeout: 30_000, intervals: [500] }).toBe("ready");

  // Start the level from level_select (Space) unless a build already boots
  // straight into gameplay.
  const initial = await readState(page);
  if (initial.screen !== "playing") {
    await injectKey(page, "Space", true);
    await injectKey(page, "Space", false);
    await expect.poll(async () => (await readState(page)).screen, {
      timeout: 10_000,
      intervals: [200],
    }).toBe("playing");
  }

  // HUD contract: a playing snapshot carries the level id, the pickup
  // counter starting empty, and a two-component finite position.
  const state = await readState(page);
  expect(state.screen).toBe("playing");
  expect(state).toHaveProperty("level");
  expect(state).toHaveProperty("collected");
  expect(state).toHaveProperty("total");
  expect(Number.isInteger(state.collected)).toBe(true);
  expect(state.collected).toBeGreaterThanOrEqual(0);
  expect(Array.isArray(state.position)).toBe(true);
  expect(state.position).toHaveLength(2);
  expect(Number.isFinite(state.position[0])).toBe(true);
  expect(Number.isFinite(state.position[1])).toBe(true);

  await page.screenshot({ path: `${screenshotRoot}/agent-level-hud.png` });
});
