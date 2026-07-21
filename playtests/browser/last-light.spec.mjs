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

async function deployReclaim(page) {
  await page.keyboard.press("Enter");
  await page.waitForTimeout(1_000);
  await page.keyboard.press("Enter");
  await page.waitForTimeout(1_000);
}

test.beforeAll(async () => {
  await mkdir(screenshotRoot, { recursive: true });
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
  await selectRosterUnit(page, 442); // Mara / Warden portrait chip.
  await page.keyboard.press("KeyR");
  await page.waitForTimeout(500);
  // Terms' Warden Hold objective focuses the ridge; the center click is the
  // authored high-ground destination after that camera focus.
  await page.mouse.click(640, 360, { button: "right" });
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
  // World (-330, 300) maps to this fixed minimap point for the 3600x2200
  // tactical map; clicking the authored contact keeps the attack probe on the
  // first Needle instead of an empty lane above it.
  await page.mouse.click(120, 657);
  await page.waitForTimeout(600);
  await page.keyboard.press("KeyA");
  await page.waitForTimeout(150);
  await page.mouse.click(640, 360, { button: "right" });
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
  expect(peakMagenta).toBeGreaterThan(80 * dpr * dpr);
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
  expect(brightPixels(contested, safeZones.protected_playfield, dpr))
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
  expect(brightPixels(deployed, safeZones.hud_regions.minimap, dpr))
    .toBeGreaterThan(450 * dpr * dpr);
  expect(brightPixels(deployed, safeZones.protected_playfield, dpr))
    .toBeGreaterThan(300 * dpr * dpr);
});
