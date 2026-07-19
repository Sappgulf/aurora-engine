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

async function canvasImage(page, clip) {
  return PNG.sync.read(await page.screenshot({ clip }));
}

async function resumeUntilCommandCardVisible(page, dpr) {
  for (let attempt = 0; attempt < 5; attempt += 1) {
    await page.keyboard.press("Escape");
    await page.waitForTimeout(1_000);
    const region = safeZones.hud_regions.command_card;
    const image = await canvasImage(page, region);
    const localRegion = { x: 0, y: 0, width: region.width, height: region.height };
    if (brightPixels(image, localRegion, dpr) > 500 * dpr * dpr) {
      return;
    }
  }
  throw new Error("Tactical pause did not resume after five frame-separated attempts");
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
  await page.waitForTimeout(750);

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
  await page.keyboard.press("Enter");
  await page.waitForTimeout(1_000);
  await page.keyboard.press("Enter");
  await page.waitForTimeout(1_000);
  await page.keyboard.press("Escape");
  await page.waitForTimeout(1_000);
  await capture(page, testInfo.project.name, "tactical-pause", dpr);

  await resumeUntilCommandCardVisible(page, dpr);
  for (let attempt = 0; attempt < 3; attempt += 1) {
    await page.keyboard.press("KeyQ");
    await page.waitForTimeout(1_000);
  }
  const production = await capture(page, testInfo.project.name, "production-active", dpr);
  expect(brightPixels(production, safeZones.hud_regions.command_card, dpr))
    .toBeGreaterThan(500 * dpr * dpr);

  expect(consoleErrors).toEqual([]);
});
