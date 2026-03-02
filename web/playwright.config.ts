import { defineConfig } from "@playwright/test";

const baseURL = process.env.E2E_BASE_URL ?? "http://127.0.0.1:60080";
const baseUrlObj = new URL(baseURL);
// Vite accepts the last `--port` flag; let E2E runs override the default 60080 safely.
const devPort = baseUrlObj.port || "60080";

export default defineConfig({
	testDir: "./tests/e2e",
	timeout: 60_000,
	expect: {
		timeout: 5_000,
	},
	retries: process.env.CI ? 2 : 0,
	reporter: [
		["list"],
		process.env.CI
			? ["html", { outputFolder: "playwright-report" }]
			: ["html", { open: "never" }],
	],
	use: {
		baseURL,
		trace: "on-first-retry",
		screenshot: "only-on-failure",
		video: "retain-on-failure",
		viewport: { width: 1280, height: 900 },
	},
	webServer: {
		command: `bun run dev -- --port ${devPort}`,
		url: baseURL,
		// Avoid silently reusing an unrelated server on the same port in local runs.
		// Opt-in via `E2E_REUSE_EXISTING_SERVER=1` when you intentionally pre-started the app.
		reuseExistingServer:
			!process.env.CI && process.env.E2E_REUSE_EXISTING_SERVER === "1",
	},
});
