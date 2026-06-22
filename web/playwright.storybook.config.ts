import { defineConfig } from "@playwright/test";

const baseURL = process.env.STORYBOOK_BASE_URL ?? "http://127.0.0.1:60081";
const baseUrlObj = new URL(baseURL);
const storybookPort = baseUrlObj.port || "60081";

export default defineConfig({
	testDir: "./tests/storybook",
	timeout: 60_000,
	expect: {
		timeout: 5_000,
	},
	retries: process.env.CI ? 2 : 0,
	reporter: [
		["list"],
		process.env.CI
			? ["html", { outputFolder: "playwright-report-storybook" }]
			: [
					"html",
					{ open: "never", outputFolder: "playwright-report-storybook" },
				],
	],
	use: {
		baseURL,
		trace: "on-first-retry",
		screenshot: "only-on-failure",
		video: "retain-on-failure",
		viewport: { width: 1600, height: 1200 },
	},
	webServer: {
		command: `bun run storybook -- --port ${storybookPort} --host 127.0.0.1`,
		url: baseURL,
		reuseExistingServer:
			!process.env.CI && process.env.E2E_REUSE_EXISTING_SERVER === "1",
	},
});
