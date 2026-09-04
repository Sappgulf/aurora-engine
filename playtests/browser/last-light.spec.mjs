import { test, expect } from "@playwright/test";
import { mkdir, readFile } from "node:fs/promises";
import { PNG } from "pngjs";
import safeZones from "../last_light/hud-safe-zones.json" with { type: "json" };

const screenshotRoot = "playtests/screenshots";

function intersects(a, b) {
  return !(
    a.x + a.width <= b.x
    || b.x + b.width <= a.x
    || a.y + a.height <= b.y
    || b.y + b.height <= a.y
  );
}

async function capture(page, projectName, checkpoint, dpr) {
  const path = `${screenshotRoot}/${projectName}-${checkpoint}.png`;
  await page.screenshot({ path });
  const png = await readFile(path);
  expect(png.subarray(1, 4).toString()).toBe("PNG");
  expect(png.readUInt32BE(16)).toBe(safeZones.viewport.width * dpr);
  expect(png.readUInt32BE(20)).toBe(safeZones.viewport.height * dpr);
  return PNG.sync.read(png);
}

// WebGPU can finish loading the WASM module before its first authored frame
// reaches the canvas. Waiting on a visible pixel keeps screenshot assertions
// deterministic on both DPR lanes without baking another arbitrary sleep into
// every campaign checkpoint.
async function waitForRenderedFrame(page) {
  await expect.poll(async () => {
    const screenshot = await page.screenshot();
    const image = PNG.sync.read(screenshot);
    for (let y = 0; y < image.height; y += 8) {
      for (let x = 0; x < image.width; x += 8) {
        const offset = (y * image.width + x) * 4;
        if (image.data[offset] + image.data[offset + 1] + image.data[offset + 2] > 450) {
          return true;
        }
      }
    }
    return false;
  }, { timeout: 20_000, intervals: [200, 500, 1_000] }).toBe(true);
}

async function samplePeakBrightPixels(page, region, dpr, samples = 10, intervalMs = 100) {
  let peak = 0;
  for (let sample = 0; sample < samples; sample += 1) {
    const image = PNG.sync.read(await page.screenshot());
    peak = Math.max(peak, brightPixels(image, region, dpr));
    if (sample + 1 < samples) await page.waitForTimeout(intervalMs);
  }
  return peak;
}

function brightPixels(image, region, dpr) {
  let count = 0;
  const left = region.x * dpr;
  const top = region.y * dpr;
  const right = (region.x + region.width) * dpr;
  const bottom = (region.y + region.height) * dpr;
  for (let y = top; y < bottom; y += 1) {
    for (let x = left; x < right; x += 1) {
      const offset = (y * image.width + x) * 4;
      if (image.data[offset] + image.data[offset + 1] + image.data[offset + 2] > 420) {
        count += 1;
      }
    }
  }
  return count;
}

function tintedPixels(image, region, dpr, predicate) {
  let count = 0;
  const left = region.x * dpr;
  const top = region.y * dpr;
  const right = (region.x + region.width) * dpr;
  const bottom = (region.y + region.height) * dpr;
  for (let y = top; y < bottom; y += 1) {
    for (let x = left; x < right; x += 1) {
      const offset = (y * image.width + x) * 4;
      if (predicate(image.data[offset], image.data[offset + 1], image.data[offset + 2])) {
        count += 1;
      }
    }
  }
  return count;
}

function magentaPixels(image, region, dpr) {
  return tintedPixels(image, region, dpr, (red, green, blue) => (
    red > 100 && red > blue * 1.15 && blue > green * 1.45
  ));
}

function redHudPixels(image, region, dpr) {
  return tintedPixels(image, region, dpr, (red, green, blue) => (
    red > 130 && red > green * 1.6 && blue > green * 1.2
  ));
}

async function installCampaignSave(page, campaign) {
  await page.addInitScript(({ key, value }) => {
    window.localStorage.setItem(key, JSON.stringify(value));
  }, { key: "aurora:last-light:campaign", value: campaign });
}

async function readAuroraState(page) {
  return page.evaluate(() => {
    if (typeof window.auroraState !== "function") {
      throw new Error("window.auroraState is unavailable in the Last Light web build");
    }
    return JSON.parse(window.auroraState());
  });
}

async function readAuroraStateWithRetry(page, timeoutMs = 2_000, retryDelayMs = 80) {
  const deadline = Date.now() + timeoutMs;
  let lastError;
  while (Date.now() < deadline) {
    try {
      return await readAuroraState(page);
    } catch (error) {
      const text = `${error.message || error}`;
      lastError = error;
      if (!/Target page, context or browser has been closed|Execution context was destroyed/.test(text)) {
        throw error;
      }
      await page.waitForTimeout(retryDelayMs);
    }
  }
  throw lastError ?? new Error("Failed to read aurora state after repeated retries");
}

async function waitForAgentInputFrame(page) {
  await page.evaluate(() => new Promise((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(resolve));
  }));
}

async function pressPad(page, button) {
  await page.evaluate(name => {
    if (typeof window.auroraInjectPad !== "function") {
      throw new Error("window.auroraInjectPad is unavailable in the Last Light web build");
    }
    window.auroraInjectPad(name, true);
  }, button);
  await waitForAgentInputFrame(page);
  await page.evaluate(name => window.auroraInjectPad(name, false), button);
  await waitForAgentInputFrame(page);
}

async function setPadStick(page, stick, x, y) {
  await page.evaluate(({ stick, x, y }) => {
    if (typeof window.auroraInjectPadStick !== "function") {
      throw new Error("window.auroraInjectPadStick is unavailable in the Last Light web build");
    }
    window.auroraInjectPadStick(stick, x, y);
  }, { stick, x, y });
  await waitForAgentInputFrame(page);
}

async function moveAgentMouse(page, x, y) {
  await page.evaluate(({ x, y }) => {
    if (typeof window.auroraInjectMouseMove !== "function") {
      throw new Error("window.auroraInjectMouseMove is unavailable in the Last Light web build");
    }
    window.auroraInjectMouseMove(x, y);
  }, { x, y });
  await waitForAgentInputFrame(page);
}

async function setAgentMouseButton(page, button, down) {
  await page.evaluate(({ button, down }) => {
    if (typeof window.auroraInjectMouseButton !== "function") {
      throw new Error("window.auroraInjectMouseButton is unavailable in the Last Light web build");
    }
    window.auroraInjectMouseButton(button, down);
  }, { button, down });
  await waitForAgentInputFrame(page);
}

async function clickAgentMouse(page, x, y, button = "Left") {
  await moveAgentMouse(page, x, y);
  await setAgentMouseButton(page, button, true);
  await setAgentMouseButton(page, button, false);
}

async function dragAgentMouse(page, start, end) {
  await moveAgentMouse(page, start.x, start.y);
  await setAgentMouseButton(page, "Left", true);
  await moveAgentMouse(page, end.x, end.y);
  await setAgentMouseButton(page, "Left", false);
}

async function callAgentGame(page, action, args = {}) {
  await page.evaluate(({ action, args }) => {
    if (typeof window.auroraGame !== "function") {
      throw new Error("window.auroraGame is unavailable in the Last Light web build");
    }
    window.auroraGame(action, JSON.stringify(args));
  }, { action, args });
  await page.waitForTimeout(120);
}

async function deploySelectedMission(page) {
  await page.keyboard.press("Enter");
  await page.waitForTimeout(700);
  await page.keyboard.press("Space");
  await page.waitForTimeout(900);
}

async function selectRosterUnit(page, portraitX) {
  // Select the whole authored roster first so the portrait chips are stable;
  // then split to one role without relying on overlapping world sprites.
  await page.mouse.move(400, 60);
  await page.mouse.down();
  await page.mouse.move(820, 410);
  await page.mouse.up();
  await page.waitForTimeout(250);
  await page.mouse.click(portraitX, 590);
  await page.waitForTimeout(250);
}

async function selectRosterUnitViaAgent(page, portraitX) {
  await dragAgentMouse(page, { x: 400, y: 60 }, { x: 820, y: 410 });
  await clickAgentMouse(page, portraitX, 590);
  await page.waitForTimeout(250);
}

async function deployReclaim(page) {
  await page.keyboard.press("Enter");
  await page.waitForTimeout(1_000);
  await page.keyboard.press("Enter");
  await page.waitForTimeout(1_000);
}

test.beforeAll(async () => {
  await mkdir(screenshotRoot, { recursive: true });
});

test("controller deploys, pauses, and opens tactical command focus", async ({ page }, testInfo) => {
  const dpr = testInfo.project.use.deviceScaleFactor;
  await page.goto("/");
  await expect(page.locator("#aurora-canvas")).toBeVisible();
  await waitForRenderedFrame(page);
  await expect.poll(async () => (await readAuroraState(page))?.screen, {
    timeout: 20_000,
    intervals: [200, 500],
  }).toBe("mission_select");

  await pressPad(page, "South");
  await expect.poll(async () => (await readAuroraState(page)).screen).toBe("briefing");

  await pressPad(page, "North");
  await expect.poll(async () => (await readAuroraState(page)).screen).toBe("tactical");

  await pressPad(page, "Start");
  await expect.poll(async () => (await readAuroraState(page)).screen).toBe("paused");

  await pressPad(page, "East");
  await expect.poll(async () => (await readAuroraState(page)).screen).toBe("tactical");

  await pressPad(page, "North");
  await expect.poll(async () => (await readAuroraState(page)).controller_command_mode)
    .toBe(true);
  await page.waitForTimeout(250);
  const state = await readAuroraState(page);
  expect(state.controller_mode).toBe(true);
  expect(state.selected).toBeGreaterThan(0);
  expect(state.controller_cursor).toHaveLength(2);
  await capture(page, testInfo.project.name, "controller-command-focus", dpr);

  await pressPad(page, "East");
  await expect.poll(async () => (await readAuroraState(page)).controller_command_mode)
    .toBe(false);
  await page.waitForTimeout(250);
  await capture(page, testInfo.project.name, "controller-tactical-cursor", dpr);
});

test("control deck edits persisted input and accessibility preferences", async ({ page }, testInfo) => {
  const dpr = testInfo.project.use.deviceScaleFactor;
  await page.goto("/");
  await expect(page.locator("#aurora-canvas")).toBeVisible();
  await waitForRenderedFrame(page);
  await deployReclaim(page);

  // The pause CTA is screen-pinned, so this click also exercises the same
  // layout geometry used by the native-facing HUD rather than a DOM shortcut.
  await page.keyboard.press("Escape");
  await expect.poll(async () => (await readAuroraState(page)).screen).toBe("paused");
  await page.mouse.click(640, 540);
  await expect.poll(async () => (await readAuroraState(page)).screen).toBe("settings");

  const initial = await readAuroraState(page);
  expect(initial.settings_open).toBe(true);
  expect(initial.settings_cursor).toBe(0);
  expect(initial.settings.cursor_sensitivity).toBeCloseTo(1.0, 3);
  const panel = await capture(page, testInfo.project.name, "control-deck", dpr);
  expect(brightPixels(panel, { x: 260, y: 150, width: 760, height: 420 }, dpr))
    .toBeGreaterThan(120 * dpr * dpr);

  await page.keyboard.press("ArrowRight");
  await expect.poll(async () => (await readAuroraState(page)).settings.cursor_sensitivity)
    .toBeCloseTo(1.25, 3);
  await page.keyboard.press("ArrowDown");
  await expect.poll(async () => (await readAuroraState(page)).settings_cursor).toBe(1);
  await page.keyboard.press("ArrowLeft");
  await expect.poll(async () => (await readAuroraState(page)).settings.dead_zone)
    .toBeCloseTo(0.15, 3);
  for (let index = 2; index <= 5; index += 1) {
    await page.keyboard.press("ArrowDown");
    await expect.poll(async () => (await readAuroraState(page)).settings_cursor).toBe(index);
  }
  await page.keyboard.press("Enter");
  await expect.poll(async () => (await readAuroraState(page)).settings.reduced_motion)
    .toBe(true);

  await page.keyboard.press("Escape");
  await expect.poll(async () => (await readAuroraState(page)).screen).toBe("paused");
  const closed = await readAuroraState(page);
  expect(closed.settings_open).toBe(false);
  expect(closed.settings.cursor_sensitivity).toBeCloseTo(1.25, 3);
  expect(closed.settings.reduced_motion).toBe(true);
});

test("controller beacon placement previews and commits at the virtual cursor", async ({ page }, testInfo) => {
  const dpr = testInfo.project.use.deviceScaleFactor;
  await page.goto("/");
  await expect(page.locator("#aurora-canvas")).toBeVisible();
  await expect.poll(() => page.title()).toBe("Aurora: Last Light");
  await waitForRenderedFrame(page);
  await deployReclaim(page);

  // Move the physical pointer away first. The controller path should then
  // make the virtual cursor the only placement coordinate source.
  await moveAgentMouse(page, 1120, 300);
  await waitForAgentInputFrame(page);
  await pressPad(page, "North");
  await expect.poll(async () => (await readAuroraState(page)).controller_command_mode)
    .toBe(true);

  // The opening roster is selected as a squad. The controller card shows
  // three rows per page, so advance to page two and focus its third row.
  await pressPad(page, "RightShoulder");
  await pressPad(page, "DpadDown");
  await pressPad(page, "DpadDown");
  await pressPad(page, "South");
  await expect.poll(async () => (await readAuroraState(page)).placing_beacon)
    .toBe(true);

  const placing = await readAuroraStateWithRetry(page);
  expect(placing.controller_mode).toBe(true);
  expect(placing.controller_command_mode).toBe(false);
  expect(placing.controller_cursor[0]).toBeCloseTo(640, 0);
  expect(placing.controller_cursor[1]).toBeCloseTo(360, 0);
  const preview = await capture(page, testInfo.project.name, "controller-beacon-placement", dpr);
  expect(brightPixels(preview, { x: 560, y: 280, width: 160, height: 160 }, dpr))
    .toBeGreaterThan(8 * dpr * dpr);

  // Exercise the real analog path too: the preview and eventual commit move
  // together when the virtual cursor leaves its center spawn point.
  await setPadStick(page, "Right", 0.7, 0.0);
  await page.waitForTimeout(180);
  await setPadStick(page, "Right", 0.0, 0.0);
  const moved = await readAuroraStateWithRetry(page, 3_000);
  expect(moved.controller_cursor[0]).toBeGreaterThan(640);

  await pressPad(page, "South");
  await expect.poll(async () => (await readAuroraState(page)).placing_beacon)
    .toBe(false);
  await expect.poll(async () => (await readAuroraState(page)).field_beacons)
    .toHaveLength(1);
  const deployed = await readAuroraStateWithRetry(page, 3_000);
  expect(deployed.field_beacons[0]).toEqual(expect.arrayContaining([
    expect.any(Number),
    expect.any(Number),
  ]));
  await capture(page, testInfo.project.name, "controller-beacon-deployed", dpr);
});

test("Reclaim checkpoints preserve the playfield at fixed DPR", async ({ page }, testInfo) => {
  const dpr = testInfo.project.use.deviceScaleFactor;
  const consoleErrors = [];
  page.on("console", message => {
    const text = message.text();
    const knownStaticProbe = text === "Failed to load resource: the server responded with a status of 404 (File not found)";
    if (message.type() === "error" && !knownStaticProbe) consoleErrors.push(text);
  });

  await page.goto("/");
  const canvas = page.locator("#aurora-canvas");
  await expect(canvas).toBeVisible();
  await expect.poll(() => page.evaluate(() => Boolean(navigator.gpu))).toBe(true);
  await expect.poll(() => page.title()).toBe("Aurora: Last Light");
  await waitForRenderedFrame(page);

  const bounds = await canvas.boundingBox();
  expect(bounds).toEqual({ x: 0, y: 0, width: 1280, height: 720 });
  const backingStore = await canvas.evaluate(element => ({
    width: element.width,
    height: element.height,
  }));
  expect(backingStore).toEqual({ width: 1280 * dpr, height: 720 * dpr });

  const regions = Object.values(safeZones.hud_regions);
  for (const region of regions) {
    expect(intersects(region, safeZones.protected_playfield)).toBe(false);
  }
  for (let left = 0; left < regions.length; left += 1) {
    for (let right = left + 1; right < regions.length; right += 1) {
      expect(intersects(regions[left], regions[right])).toBe(false);
    }
  }

  await capture(page, testInfo.project.name, "mission-select", dpr);
  await deployReclaim(page);
  await page.keyboard.press("Escape");
  await page.waitForTimeout(1_000);
  await capture(page, testInfo.project.name, "tactical-pause", dpr);

  await page.reload();
  await page.waitForTimeout(1_000);
  await deployReclaim(page);
  // Exercise the RTS command vocabulary before production: attack-move,
  // patrol, and a queued waypoint must all be accepted without a browser-side
  // exception even when the click lands on open terrain.
  await page.keyboard.press("KeyA");
  await page.mouse.click(820, 360, { button: "right" });
  await page.keyboard.press("KeyP");
  await page.mouse.click(640, 300, { button: "right" });
  await page.keyboard.down("Shift");
  await page.mouse.click(720, 420, { button: "right" });
  await page.keyboard.up("Shift");
  // The contextual card must expose and execute the selected specialist's
  // signature action, and Space should re-center the most recent comms ping.
  await page.keyboard.press("KeyY");
  await page.waitForTimeout(250);
  await page.keyboard.press("Space");
  await page.waitForTimeout(250);
  await page.waitForTimeout(500);
  await page.keyboard.press("KeyQ");
  await page.waitForTimeout(2_000);
  await page.keyboard.press("KeyQ");
  await page.waitForTimeout(500);
  const production = await capture(page, testInfo.project.name, "production-active", dpr);
  // Compact bitmap headers and browser anti-aliasing vary slightly by DPR;
  // keep the assertion focused on a clearly rendered command card rather than
  // one exact glyph-cell count.
  expect(brightPixels(production, safeZones.hud_regions.command_card, dpr))
    .toBeGreaterThan(300 * dpr * dpr);

  expect(consoleErrors).toEqual([]);
});

test("Bell Mine detonation reads as a one-shot combat event", async ({ page }, testInfo) => {
  const dpr = testInfo.project.use.deviceScaleFactor;
  await page.goto("/");
  await expect(page.locator("#aurora-canvas")).toBeVisible();
  await expect.poll(() => page.title()).toBe("Aurora: Last Light");
  await waitForRenderedFrame(page);
  await deployReclaim(page);

  await expect.poll(async () => (await readAuroraState(page)).screen).toBe("tactical");
  const detonationsBefore = (await readAuroraState(page)).combat.detonation_count;
  await callAgentGame(page, "engage_kind", {
    // The Warden's long reach intentionally lets it dismantle a Bell Mine
    // from outside the trigger envelope. Use the Engineer's shorter reach so
    // this scenario exercises the authored close-range counterplay.
    attacker: "engineer",
    target: "bell_mine",
  });

  await expect.poll(async () => (await readAuroraState(page)).combat.engagement?.target, {
    timeout: 3_000,
    intervals: [25, 50, 100],
  }).not.toBeNull();
  await expect.poll(async () => (await readAuroraState(page)).combat.detonation_count, {
    timeout: dpr > 1 ? 30_000 : 15_000,
    intervals: [25, 50, 100, 250],
  }).toBeGreaterThan(detonationsBefore);
  await expect.poll(async () => (await readAuroraState(page)).combat.detonations, {
    timeout: 2_000,
    intervals: [16, 25, 40],
  }).toBeGreaterThan(0);

  const blast = await capture(page, testInfo.project.name, "bell-mine-detonation", dpr);
  expect(brightPixels(blast, safeZones.protected_playfield, dpr))
    .toBeGreaterThan(300 * dpr * dpr);

  await expect.poll(async () => (await readAuroraState(page)).combat.detonations, {
    timeout: 2_000,
    intervals: [50, 100, 200],
  }).toBe(0);
  await expect.poll(async () => (await readAuroraState(page)).combat.wrecks, {
    timeout: 2_000,
    intervals: [50, 100, 200],
  }).toBeGreaterThan(0);
});

test("Tier-five campaign exposes the Verdant chapter without HUD overflow", async ({ page }, testInfo) => {
  const dpr = testInfo.project.use.deviceScaleFactor;
  const campaignSave = {
    format_version: 1,
    payload: {
      runs_completed: 3,
      campaign: {
        unlocked_mission: 5,
        completed_missions: ["reclaim-the-reactor", "voice-in-conduit-twelve", "terms-of-salvage"],
        currency: 210,
        decisions: ["meridian-allied"],
        upgrades: [],
        specialist_loadouts: [],
      },
    },
  };
  await page.addInitScript(({ key, value }) => {
    window.localStorage.setItem(key, JSON.stringify(value));
  }, { key: "aurora:last-light:campaign", value: campaignSave });
  await page.goto("/");
  await expect(page.locator("#aurora-canvas")).toBeVisible();
  await expect.poll(() => page.title()).toBe("Aurora: Last Light");
  await waitForRenderedFrame(page);
  const screenshot = await capture(page, testInfo.project.name, "mission-select-tier5", dpr);
  expect(brightPixels(screenshot, { x: 210, y: 170, width: 860, height: 360 }, dpr))
    .toBeGreaterThan(2_000 * dpr * dpr);
  await page.keyboard.press("Enter");
  await page.waitForTimeout(500);
  const briefing = await capture(page, testInfo.project.name, "garden-briefing", dpr);
  expect(brightPixels(briefing, { x: 120, y: 80, width: 1_000, height: 520 }, dpr))
    .toBeGreaterThan(2_000 * dpr * dpr);
  await page.keyboard.press("Space");
  await page.waitForTimeout(750);
  const deployed = await capture(page, testInfo.project.name, "garden-deployed", dpr);
  expect(brightPixels(deployed, safeZones.protected_playfield, dpr))
    .toBeGreaterThan(300 * dpr * dpr);
  // The tactical hint and startup toast are intentionally transient. Capture
  // the settled state as well so a future HUD change cannot silently turn
  // onboarding copy into permanent playfield chrome.
  await page.waitForTimeout(4_500);
  const quiet = await capture(page, testInfo.project.name, "garden-quiet", dpr);
  expect(brightPixels(quiet, safeZones.protected_playfield, dpr))
    .toBeGreaterThan(300 * dpr * dpr);
});

test("Terms ridge progress and attack-move telegraph stay visible", async ({ page }, testInfo) => {
  const dpr = testInfo.project.use.deviceScaleFactor;
  await installCampaignSave(page, {
    format_version: 1,
    payload: {
      runs_completed: 3,
      campaign: {
        unlocked_mission: 4,
        completed_missions: ["reclaim-the-reactor", "voice-in-conduit-twelve"],
        currency: 300,
        decisions: ["lumen-contact-established"],
        upgrades: [],
        specialist_loadouts: [],
      },
    },
  });

  await page.goto("/");
  await expect(page.locator("#aurora-canvas")).toBeVisible();
  await expect.poll(() => page.title()).toBe("Aurora: Last Light");
  await waitForRenderedFrame(page);
  await deploySelectedMission(page);
  await selectRosterUnitViaAgent(page, 442); // Mara / Warden portrait chip.
  await page.keyboard.press("KeyR");
  await page.waitForTimeout(500);
  // Terms' Warden Hold objective focuses the ridge; the center click is the
  // authored high-ground destination after that camera focus.
  await clickAgentMouse(page, 640, 360, "Right");
  await page.waitForTimeout(4_500);
  const holding = await capture(page, testInfo.project.name, "terms-ridge-holding", dpr);
  expect(brightPixels(holding, safeZones.hud_regions.objective, dpr))
    .toBeGreaterThan(1_500 * dpr * dpr);
  expect(brightPixels(holding, safeZones.protected_playfield, dpr))
    .toBeGreaterThan(300 * dpr * dpr);

  // Reframe on the authored first Needle through the minimap, then use the
  // same attack-move command a player would use. Poll a short window because
  // the player impact pulse is intentionally brief while the enemy telegraph
  // remains readable for several frames.
  // Expanded world (-422, 384) maps to this fixed minimap point for the 3600x2200
  // tactical map; clicking the authored contact keeps the attack probe on the
  // first Needle instead of an empty lane above it.
  await clickAgentMouse(page, 120, 614);
  await page.waitForTimeout(600);
  const pulsesBeforeOrder = (await readAuroraState(page)).combat.pulses;
  await page.keyboard.press("KeyA");
  await page.waitForTimeout(150);
  await clickAgentMouse(page, 640, 360, "Right");
  // Attack-move validates the stance and route first. The domain-level agent
  // action then asks the selected Warden to engage the nearby authored Needle via
  // the same simulation order path, avoiding a moving screen-space target.
  await callAgentGame(page, "engage_kind", {
    attacker: "warden",
    target: "needle",
  });
  try {
    await expect.poll(async () => (await readAuroraState(page)).combat.pulses, {
      timeout: dpr > 1 ? 45_000 : 12_000,
      intervals: [25, 50, 100],
    }).toBeGreaterThan(pulsesBeforeOrder);
  } catch (error) {
    const state = await readAuroraState(page);
    throw new Error(`${error.message}\nLast Light agent state: ${JSON.stringify(state)}`);
  }
  await expect.poll(async () => {
    const combat = (await readAuroraState(page)).combat;
    return combat.active_shots + combat.active_impacts;
  }, {
    timeout: 2_000,
    intervals: [16, 25, 40],
  }).toBeGreaterThan(0);
  await capture(page, testInfo.project.name, "terms-combat-impact", dpr);
  let peakMagenta = 0;
  for (let sample = 0; sample < 10; sample += 1) {
    await page.waitForTimeout(100);
    const image = await capture(page, testInfo.project.name, `terms-attack-${sample}`, dpr);
    // The authored enemy can sit on the right edge of the tactical lane,
    // immediately beside (but above) the command card. Keep the probe out of
    // the HUD while allowing that edge telegraph to be counted.
    const telegraphLane = { x: 280, y: 210, width: 990, height: 300 };
    peakMagenta = Math.max(peakMagenta, magentaPixels(image, telegraphLane, dpr));
  }
  expect(peakMagenta).toBeGreaterThan(24 * dpr * dpr);
});

test("Garden node contest publishes the red resource objective chip", async ({ page }, testInfo) => {
  const dpr = testInfo.project.use.deviceScaleFactor;
  await installCampaignSave(page, {
    format_version: 1,
    payload: {
      runs_completed: 3,
      campaign: {
        unlocked_mission: 5,
        completed_missions: [
          "reclaim-the-reactor",
          "voice-in-conduit-twelve",
          "terms-of-salvage",
        ],
        currency: 300,
        decisions: ["meridian-allied"],
        upgrades: [],
        specialist_loadouts: [],
      },
    },
  });

  await page.goto("/");
  await expect(page.locator("#aurora-canvas")).toBeVisible();
  await expect.poll(() => page.title()).toBe("Aurora: Last Light");
  await waitForRenderedFrame(page);
  await deploySelectedMission(page);
  await selectRosterUnit(page, 538); // Sena / Surveyor portrait chip.

  // Garden node 2 is the authored contest pocket. The minimap coordinate is
  // stable because it comes from the fixed MAP_SIZE/panel contract; clicking
  // its center then assigns the selected Surveyor to the live resource node.
  await page.mouse.click(130, 621);
  await page.waitForTimeout(500);
  await page.mouse.click(640, 360);
  await page.waitForTimeout(5_500);
  const contested = await capture(page, testInfo.project.name, "garden-node-contested", dpr);
  expect(redHudPixels(contested, safeZones.hud_regions.objective, dpr))
    .toBeGreaterThan(80 * dpr * dpr);
  // Garden's reactor beam pulses through this region. Sample one pulse window
  // so the assertion measures authored scene presence rather than one phase of
  // the animation; the red chip above still carries the objective-state check.
  const peakPlayfield = Math.max(
    brightPixels(contested, safeZones.protected_playfield, dpr),
    await samplePeakBrightPixels(page, safeZones.protected_playfield, dpr),
  );
  expect(peakPlayfield)
    .toBeGreaterThan(300 * dpr * dpr);
});

test("Mission seven opens the Vesper Gate branch and tactical gate HUD", async ({ page }, testInfo) => {
  const dpr = testInfo.project.use.deviceScaleFactor;
  await installCampaignSave(page, {
    format_version: 1,
    payload: {
      runs_completed: 5,
      campaign: {
        unlocked_mission: 7,
        completed_missions: [
          "reclaim-the-reactor",
          "voice-in-conduit-twelve",
          "terms-of-salvage",
          "garden-below",
          "choir-invisible",
        ],
        currency: 600,
        decisions: [
          "lumen-contact-established",
          "meridian-allied",
          "verdant-cultivated",
          "choir-invisible-cleared",
        ],
        upgrades: [],
        specialist_loadouts: [],
      },
    },
  });

  await page.goto("/");
  await expect(page.locator("#aurora-canvas")).toBeVisible();
  await expect.poll(() => page.title()).toBe("Aurora: Last Light");
  await waitForRenderedFrame(page);
  const missionSelect = await capture(page, testInfo.project.name, "vesper-gate-select", dpr);
  expect(brightPixels(missionSelect, { x: 210, y: 170, width: 860, height: 420 }, dpr))
    .toBeGreaterThan(2_500 * dpr * dpr);

  await page.keyboard.press("Enter");
  await page.waitForTimeout(500);
  const briefing = await capture(page, testInfo.project.name, "vesper-gate-briefing", dpr);
  expect(brightPixels(briefing, { x: 120, y: 80, width: 1_000, height: 520 }, dpr))
    .toBeGreaterThan(2_500 * dpr * dpr);

  await page.keyboard.press("Space");
  await page.waitForTimeout(900);
  const deployed = await capture(page, testInfo.project.name, "vesper-gate-deployed", dpr);
  expect(brightPixels(deployed, safeZones.hud_regions.objective, dpr))
    .toBeGreaterThan(1_500 * dpr * dpr);
  expect(brightPixels(deployed, safeZones.hud_regions.minimap, dpr))
    .toBeGreaterThan(400 * dpr * dpr);
  expect(brightPixels(deployed, safeZones.protected_playfield, dpr))
    .toBeGreaterThan(300 * dpr * dpr);
});

test("Mission eight opens the Hollow Orbit branch and three-role objective HUD", async ({ page }, testInfo) => {
  const dpr = testInfo.project.use.deviceScaleFactor;
  await installCampaignSave(page, {
    format_version: 1,
    payload: {
      runs_completed: 6,
      campaign: {
        unlocked_mission: 8,
        completed_missions: [
          "reclaim-the-reactor",
          "voice-in-conduit-twelve",
          "terms-of-salvage",
          "garden-below",
          "choir-invisible",
          "vesper-gate",
        ],
        currency: 800,
        decisions: [
          "lumen-contact-established",
          "lumen-awakened",
          "meridian-allied",
          "verdant-cultivated",
          "choir-invisible-cleared",
          "vesper-gate-open",
        ],
        upgrades: [],
        specialist_loadouts: [],
      },
    },
  });

  await page.goto("/");
  await expect(page.locator("#aurora-canvas")).toBeVisible();
  await expect.poll(() => page.title()).toBe("Aurora: Last Light");
  await waitForRenderedFrame(page);
  // The game initializes the cursor to the newest unlocked entry. With tier 8
  // this is index 6, the seventh mission, The Hollow Orbit.
  const missionSelect = await capture(page, testInfo.project.name, "hollow-orbit-select", dpr);
  expect(brightPixels(missionSelect, { x: 210, y: 170, width: 860, height: 460 }, dpr))
    .toBeGreaterThan(2_500 * dpr * dpr);

  await page.keyboard.press("Enter");
  await page.waitForTimeout(500);
  const briefing = await capture(page, testInfo.project.name, "hollow-orbit-briefing", dpr);
  // The title, speaker card, and three-line Hollow Orbit briefing occupy this
  // overlay lane; a blank/locked mission would not produce this density.
  expect(brightPixels(briefing, { x: 120, y: 80, width: 1_000, height: 520 }, dpr))
    .toBeGreaterThan(2_500 * dpr * dpr);

  await page.keyboard.press("Space");
  await page.waitForTimeout(900);
  const deployed = await capture(page, testInfo.project.name, "hollow-orbit-deployed", dpr);
  // Mission 8 opens on the Warden's eastern-ridge hold; the Surveyor cache
  // chip is rendered separately while the Engineer coolant repair follows.
  // Both role contracts remain visible without expanding the persistent HUD.
  expect(brightPixels(deployed, safeZones.hud_regions.objective, dpr))
    .toBeGreaterThan(1_500 * dpr * dpr);
  // Minimap sprites are filtered differently at DPR2 and include moving units
  // plus a pulsing raid marker. Keep this as a structural floor rather than a
  // density target; the objective and protected-playfield probes cover the
  // mission-state contract around it.
  expect(brightPixels(deployed, safeZones.hud_regions.minimap, dpr))
    .toBeGreaterThan(400 * dpr * dpr);
  expect(brightPixels(deployed, safeZones.protected_playfield, dpr))
    .toBeGreaterThan(300 * dpr * dpr);
});
