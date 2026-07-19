# Aurora browser playtests

The Last Light browser lane captures the real Trunk application at a fixed
1280×720 CSS viewport under device-pixel ratios 1 and 2.

```bash
./scripts/build-web.sh
npm ci
npx playwright install chromium
npm run test:browser
```

The test exercises three stable checkpoints: mission select, tactical pause,
and active Warden production. It verifies WebGPU availability, canvas CSS and
backing-store dimensions, PNG dimensions, HUD safe-zone separation, visible
command-card content, and the absence of browser console errors.

Screenshots are written to `playtests/screenshots/`. They are local/CI evidence
rather than source assets, so Git ignores them and CI uploads them as the
`last-light-browser-evidence` artifact.
