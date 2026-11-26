import { expect, test } from "@playwright/test";

const STORIES = [
	"components-tcpconnectionusageview--default",
	"components-tcpconnectionusageview--long-term-30d",
];

for (const story of STORIES) {
	test(`captures ${story}`, async ({ page }) => {
		await page.setViewportSize({ width: 1000, height: 900 });
		await page.goto(`/iframe.html?id=${story}&viewMode=story`, {
			waitUntil: "networkidle",
		});
		const surface = page.locator("[data-visual-evidence-surface]");
		await expect(surface).toBeVisible();
		await surface.screenshot({ path: `.evidence/${story}.png` });
	});
}
