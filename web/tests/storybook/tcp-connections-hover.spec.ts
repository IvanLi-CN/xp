import { expect, test } from "@playwright/test";

type ChartPathSnapshot = {
	dLen: number;
	fill: string;
	stroke: string;
};

const TCP_CONNECTIONS_STORY_ID = "pages-nodedetailspage--tcp-connections-tab";

function tcpStoryUrl(theme: "dark" | "light") {
	const globals = `theme:${theme};density:comfortable`;
	return `/iframe.html?viewMode=story&id=${TCP_CONNECTIONS_STORY_ID}&globals=${globals}`;
}

function hasVisibleStroke(paths: ChartPathSnapshot[]): boolean {
	return paths.some((path) => path.stroke !== "none");
}

function hasVisibleFill(paths: ChartPathSnapshot[]): boolean {
	return paths.some((path) => path.fill !== "none");
}

async function colorChannels(
	page: import("@playwright/test").Page,
	color: string,
) {
	return page.evaluate((value) => {
		const probe = document.createElement("span");
		probe.style.color = value;
		document.body.append(probe);
		const resolved = getComputedStyle(probe).color;
		probe.remove();
		const canvas = document.createElement("canvas");
		canvas.width = 1;
		canvas.height = 1;
		const context = canvas.getContext("2d", { willReadFrequently: true });
		if (!context) throw new Error("Canvas color context is unavailable");
		context.fillStyle = resolved;
		context.fillRect(0, 0, 1, 1);
		return [...context.getImageData(0, 0, 1, 1).data];
	}, color);
}

async function visibleTooltipSurface(page: import("@playwright/test").Page) {
	return page.evaluate(() => {
		const candidates = [
			...document.querySelectorAll<HTMLElement>(".echarts-for-react div"),
		]
			.map((element) => {
				const style = getComputedStyle(element);
				const rect = element.getBoundingClientRect();
				return {
					background: style.backgroundColor,
					color: style.color,
					height: rect.height,
					text: element.textContent ?? "",
					visible: style.display !== "none" && style.visibility !== "hidden",
					width: rect.width,
				};
			})
			.filter(
				(candidate) =>
					candidate.visible &&
					candidate.text.includes("Total") &&
					candidate.background !== "rgba(0, 0, 0, 0)",
			)
			.sort(
				(left, right) => right.width * right.height - left.width * left.height,
			);

		return candidates[0] ?? null;
	});
}

for (const theme of ["dark", "light"] as const) {
	test(`tcp connections chart keeps visible paths and themes the tooltip in ${theme} mode`, async ({
		page,
	}) => {
		await page.goto(tcpStoryUrl(theme), { waitUntil: "networkidle" });

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
		await expect.poll(() => visibleTooltipSurface(page)).not.toBeNull();

		const tooltip = await visibleTooltipSurface(page);
		expect(tooltip).not.toBeNull();
		expect(await colorChannels(page, tooltip?.background ?? "")).toEqual(
			await colorChannels(page, "var(--popover)"),
		);
		expect(await colorChannels(page, tooltip?.color ?? "")).toEqual(
			await colorChannels(page, "var(--popover-foreground)"),
		);

		const afterHover = await snapshotPaths();
		expect(hasVisibleStroke(afterHover)).toBe(true);
		expect(hasVisibleFill(afterHover)).toBe(true);
	});
}
