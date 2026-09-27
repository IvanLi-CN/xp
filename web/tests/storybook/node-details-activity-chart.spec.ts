import { expect, test } from "@playwright/test";

const MOBILE_VIEWPORTS = [
	{ width: 320, height: 852 },
	{ width: 360, height: 800 },
	{ width: 393, height: 852 },
] as const;

function storyUrl() {
	return [
		"/iframe.html?viewMode=story&id=pages-nodedetailspage--default",
		"&globals=theme:light;density:comfortable",
	].join("");
}

async function activityMetrics(page: import("@playwright/test").Page) {
	return page.evaluate(() => {
		const chart = document.querySelector<HTMLElement>(
			'[data-testid="node-runtime-activity-chart-scroll"]',
		);
		const rightButton = document.querySelector<HTMLButtonElement>(
			'button[aria-label="Scroll activity chart right"]',
		);
		if (!chart || !rightButton) {
			throw new Error("Runtime activity chart controls are missing");
		}
		return {
			bodyScrollWidth: document.body.scrollWidth,
			chartClientWidth: chart.clientWidth,
			chartScrollLeft: chart.scrollLeft,
			chartScrollWidth: chart.scrollWidth,
			documentScrollWidth: document.documentElement.scrollWidth,
			innerWidth: window.innerWidth,
			rightButtonDisabled: rightButton.disabled,
			rightButtonVisible: rightButton.offsetParent !== null,
		};
	});
}

test.describe("Node details runtime activity chart", () => {
	for (const viewport of MOBILE_VIEWPORTS) {
		test(`${viewport.width}x${viewport.height} keeps timeline data in a local scroll region`, async ({
			page,
		}) => {
			await page.setViewportSize(viewport);
			await page.goto(storyUrl(), { waitUntil: "networkidle" });

			const chart = page.getByRole("region", {
				name: "7-day service activity chart",
			});
			await expect(chart).toBeVisible();
			await expect(page.getByText("00:00", { exact: true })).toBeVisible();
			await expect(page.getByText("24:00", { exact: true })).toBeVisible();

			const before = await activityMetrics(page);
			expect(before.innerWidth).toBe(viewport.width);
			expect(before.bodyScrollWidth).toBeLessThanOrEqual(viewport.width);
			expect(before.documentScrollWidth).toBeLessThanOrEqual(viewport.width);
			expect(before.chartScrollWidth).toBeGreaterThan(before.chartClientWidth);
			expect(before.rightButtonVisible).toBe(true);
			expect(before.rightButtonDisabled).toBe(false);

			await page
				.getByRole("button", { name: "Scroll activity chart right" })
				.click();
			await expect
				.poll(() =>
					activityMetrics(page).then((metrics) => metrics.chartScrollLeft),
				)
				.toBeGreaterThan(0);

			const dateLabelBox = await chart
				.locator("span.font-mono")
				.first()
				.boundingBox();
			const chartBox = await chart.boundingBox();
			if (!dateLabelBox || !chartBox) {
				throw new Error("Runtime activity chart geometry is unavailable");
			}
			expect(dateLabelBox.x).toBeGreaterThanOrEqual(chartBox.x - 1);
			expect(dateLabelBox.x + dateLabelBox.width).toBeLessThanOrEqual(
				chartBox.x + chartBox.width + 1,
			);

			await page
				.getByRole("button", { name: "Scroll activity chart left" })
				.click();
			await expect
				.poll(() =>
					activityMetrics(page).then((metrics) => metrics.chartScrollLeft),
				)
				.toBe(0);
		});
	}

	test("keeps the desktop timeline dense without page overflow", async ({
		page,
	}) => {
		await page.setViewportSize({ width: 1280, height: 900 });
		await page.goto(storyUrl(), { waitUntil: "networkidle" });

		const chart = page.getByRole("region", {
			name: "7-day service activity chart",
		});
		await expect(chart).toBeVisible();
		const metrics = await activityMetrics(page);

		expect(metrics.innerWidth).toBe(1280);
		expect(metrics.bodyScrollWidth).toBeLessThanOrEqual(1280);
		expect(metrics.documentScrollWidth).toBeLessThanOrEqual(1280);
		expect(metrics.chartScrollWidth).toBeLessThanOrEqual(
			metrics.chartClientWidth,
		);
		expect(metrics.rightButtonVisible).toBe(false);
	});
});
