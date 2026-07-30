import { expect, test } from "@playwright/test";

type SvgPathSnapshot = {
	dLen: number;
	fill: string;
	stroke: string;
};

function trafficStoryUrl(storyId: string, theme: "dark" | "light") {
	const globals = `theme:${theme};density:comfortable`;
	return `/iframe.html?viewMode=story&id=${storyId}&globals=${globals}`;
}

function hasVisibleStroke({ stroke }: SvgPathSnapshot) {
	return stroke !== "none" && stroke !== "transparent" && stroke !== "";
}

function hasVisibleFill({ fill }: SvgPathSnapshot) {
	return fill !== "none" && fill !== "transparent" && fill !== "";
}

async function chartPaths(page: import("@playwright/test").Page) {
	return page.locator(".echarts-for-react svg path").evaluateAll((paths) =>
		paths.map((path) => ({
			dLen: (path.getAttribute("d") ?? "").length,
			fill: path.getAttribute("fill") ?? "",
			stroke: path.getAttribute("stroke") ?? "",
		})),
	);
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
					boxShadow: style.boxShadow,
					color: style.color,
					height: rect.height,
					left: rect.left,
					right: rect.right,
					text: element.textContent ?? "",
					visible: style.display !== "none" && style.visibility !== "hidden",
					width: rect.width,
				};
			})
			.filter(
				(candidate) =>
					candidate.visible &&
					candidate.text.includes("Current") &&
					candidate.background !== "rgba(0, 0, 0, 0)",
			)
			.sort(
				(left, right) => right.width * right.height - left.width * left.height,
			);

		return candidates[0] ?? null;
	});
}

test("renders the deterministic Traffic tooltip preview story", async ({
	page,
}) => {
	await page.goto(
		trafficStoryUrl("components-trafficview--tooltip-preview", "dark"),
	);
	await expect(page.getByTestId("traffic-tooltip-preview")).toBeVisible();
});

test("does not add a colored evidence frame to TrafficView", async ({
	page,
}) => {
	await page.goto(
		trafficStoryUrl("components-trafficview--last-24-hours", "dark"),
	);
	await expect(
		page.getByRole("heading", { name: "Traffic", exact: true }),
	).toBeVisible({ timeout: 15_000 });
	const evidenceFrameClasses = await page.evaluate(() =>
		[...document.querySelectorAll<HTMLElement>("[class]")]
			.map((element) => element.className)
			.filter(
				(className) =>
					typeof className === "string" &&
					className.includes("bg-foreground/20"),
			),
	);
	expect(evidenceFrameClasses).toEqual([]);
});

test("confines the real Traffic tooltip within a mobile viewport", async ({
	page,
}) => {
	await page.setViewportSize({ width: 320, height: 844 });
	await page.goto(
		trafficStoryUrl("components-trafficview--last-24-hours", "dark"),
	);
	const chart = page.locator(".echarts-for-react svg");
	await expect(chart).toBeVisible({ timeout: 15_000 });
	const box = await chart.boundingBox();
	expect(box).not.toBeNull();
	await page.mouse.move(box.x + box.width * 0.58, box.y + box.height * 0.42);
	await expect.poll(() => visibleTooltipSurface(page)).not.toBeNull();

	const tooltip = await visibleTooltipSurface(page);
	expect(tooltip?.left).toBeGreaterThanOrEqual(0);
	expect(tooltip?.right).toBeLessThanOrEqual(320);
});

for (const theme of ["dark", "light"] as const) {
	test(`keeps Traffic SVG paths visible and themes the tooltip in ${theme} mode`, async ({
		page,
	}) => {
		await page.goto(
			trafficStoryUrl("components-trafficview--last-24-hours", theme),
		);

		const chart = page.locator(".echarts-for-react svg");
		await expect(chart).toBeVisible({ timeout: 15_000 });
		const beforeHover = (await chartPaths(page)).filter(
			(path) => path.dLen > 100,
		);
		expect(beforeHover.some(hasVisibleStroke)).toBe(true);
		expect(beforeHover.some(hasVisibleFill)).toBe(true);

		const box = await chart.boundingBox();
		expect(box).not.toBeNull();
		await page.mouse.move(box.x + box.width * 0.58, box.y + box.height * 0.42);
		await expect.poll(() => visibleTooltipSurface(page)).not.toBeNull();

		const tooltip = await visibleTooltipSurface(page);
		expect(tooltip).not.toBeNull();
		expect(await colorChannels(page, tooltip?.background ?? "")).toEqual(
			await colorChannels(page, "var(--popover)"),
		);
		expect(await colorChannels(page, tooltip?.color ?? "")).toEqual(
			await colorChannels(page, "var(--popover-foreground)"),
		);
		if (theme === "light") {
			expect(tooltip?.boxShadow).toContain("0px 4px 12px");
		}

		const afterHover = (await chartPaths(page)).filter(
			(path) => path.dLen > 100,
		);
		expect(afterHover.some(hasVisibleStroke)).toBe(true);
		expect(afterHover.some(hasVisibleFill)).toBe(true);
	});
}
