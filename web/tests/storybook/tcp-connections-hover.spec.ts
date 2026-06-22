import { expect, test } from "@playwright/test";

type ChartPathSnapshot = {
	dLen: number;
	fill: string;
	stroke: string;
};

function hasVisibleStroke(paths: ChartPathSnapshot[]): boolean {
	return paths.some((path) => path.stroke !== "none");
}

function hasVisibleFill(paths: ChartPathSnapshot[]): boolean {
	return paths.some((path) => path.fill !== "none");
}

test("tcp connections chart keeps visible line and area after tooltip hover", async ({
	page,
}) => {
	await page.goto(
		"/iframe.html?viewMode=story&id=pages-nodedetailspage--tcp-connections-tab",
		{ waitUntil: "networkidle" },
	);

	await expect(page.getByText("TCP connection count")).toBeVisible();
	const chart = page.locator(".echarts-for-react").first();
	await expect(chart).toBeVisible();
	const svg = chart.locator("svg");
	await expect(svg).toBeVisible();

	const snapshotPaths = async () =>
		page.evaluate(() => {
			const chartSvg = document.querySelector(".echarts-for-react svg");
			if (!chartSvg) {
				throw new Error("TCP chart svg not found");
			}
			return Array.from(chartSvg.querySelectorAll("path"))
				.map((path) => ({
					dLen: (path.getAttribute("d") ?? "").length,
					fill: getComputedStyle(path).fill,
					stroke: getComputedStyle(path).stroke,
				}))
				.filter((path) => path.dLen > 100);
		});

	const beforeHover = await snapshotPaths();
	expect(hasVisibleStroke(beforeHover)).toBe(true);
	expect(hasVisibleFill(beforeHover)).toBe(true);

	const box = await svg.boundingBox();
	if (!box) {
		throw new Error("TCP chart bounding box not available");
	}
	await page.mouse.move(box.x + box.width * 0.88, box.y + box.height * 0.3);
	await page.waitForTimeout(300);

	const afterHover = await snapshotPaths();
	expect(hasVisibleStroke(afterHover)).toBe(true);
	expect(hasVisibleFill(afterHover)).toBe(true);
});
