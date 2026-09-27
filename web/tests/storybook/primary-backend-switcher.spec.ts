import { type Page, expect, test } from "@playwright/test";

import { fixtureCatalog } from "../../src/fixture-policy/catalog";

function storyUrl(storyId: string) {
	return `/iframe.html?viewMode=story&id=${storyId}&globals=theme:dark;density:comfortable`;
}

const mobileViewports = [
	{ width: 320, height: 852 },
	{ width: 360, height: 800 },
	{ width: 393, height: 852 },
] as const;

const appShellActionLabels = [
	"Open status",
	"Open settings",
	"Open primary backend",
] as const;

type Rect = {
	bottom: number;
	height: number;
	left: number;
	right: number;
	top: number;
	width: number;
};

async function appShellLayout(page: Page) {
	return page.evaluate((actionLabels) => {
		const getRect = (element: Element | null) => {
			if (!element) return null;
			const rect = element.getBoundingClientRect();
			return {
				bottom: rect.bottom,
				height: rect.height,
				left: rect.left,
				right: rect.right,
				top: rect.top,
				width: rect.width,
			};
		};
		const getButtonRect = (label: string) =>
			getRect(document.querySelector(`header button[aria-label="${label}"]`));

		const versionButton = document.querySelector("header button.xp-badge");
		const commandButton = document.querySelector(
			'header button[aria-label="Open command palette"]',
		);
		const actionRects = actionLabels.map((label) => {
			const rect = getButtonRect(label);
			if (!rect) throw new Error(`Missing ${label}`);
			return { label, rect };
		});
		if (!versionButton) throw new Error("Missing version indicator");

		return {
			viewportWidth: window.innerWidth,
			clientWidth: document.documentElement.clientWidth,
			scrollWidth: document.documentElement.scrollWidth,
			bodyScrollWidth: document.body.scrollWidth,
			header: getRect(document.querySelector("header")),
			brand: getRect(document.querySelector("header > div > div:first-child")),
			command: getRect(commandButton),
			version: getRect(versionButton),
			actions: actionRects,
		};
	}, appShellActionLabels);
}

function expectInsideViewport(rect: Rect | null, viewportWidth: number) {
	if (!rect) throw new Error("Element has no bounding box");
	expect(rect.left).toBeGreaterThanOrEqual(0);
	expect(rect.right).toBeLessThanOrEqual(viewportWidth);
}

function expectNonOverlapping(rects: Array<Rect | null>) {
	const visibleRects = rects.filter((rect): rect is Rect => Boolean(rect));
	for (let index = 1; index < visibleRects.length; index += 1) {
		expect(visibleRects[index]?.left).toBeGreaterThanOrEqual(
			visibleRects[index - 1]?.right ?? 0,
		);
	}
}

test("shows the verified backend list from the AppShell switcher", async ({
	page,
}) => {
	await page.goto(storyUrl("components-primarybackendswitcher--default"), {
		waitUntil: "networkidle",
	});
	await page.getByRole("button", { name: "Open primary backend" }).click();

	await expect(page.getByRole("menu")).toBeVisible();
	await expect(
		page.getByRole("menuitem", {
			name: fixtureCatalog.identifier.nodeNameSecondary(),
		}),
	).toBeVisible();
	await expect(
		page.getByRole("menuitem", { name: /Current page/ }),
	).toBeDisabled();
});

test("keeps the switcher menu inside a narrow viewport", async ({ page }) => {
	for (const viewport of mobileViewports) {
		await page.setViewportSize(viewport);
		await page.goto(storyUrl("components-primarybackendswitcher--default"), {
			waitUntil: "networkidle",
		});
		await page.getByRole("button", { name: "Open primary backend" }).click();

		const menu = page.getByRole("menu");
		await expect(menu).toBeVisible();
		const box = await menu.boundingBox();
		if (!box) throw new Error("Primary backend menu has no bounding box");
		expect(box.x).toBeGreaterThanOrEqual(0);
		expect(box.x + box.width).toBeLessThanOrEqual(viewport.width);
		await page.keyboard.press("Escape");
	}
});

test("keeps AppShell header controls inside narrow mobile rows", async ({
	page,
}) => {
	for (const viewport of mobileViewports) {
		await page.setViewportSize(viewport);
		await page.goto(storyUrl("components-appshell--default"), {
			waitUntil: "networkidle",
		});

		const layout = await appShellLayout(page);
		expect(layout.scrollWidth).toBeLessThanOrEqual(layout.clientWidth);
		expect(layout.bodyScrollWidth).toBeLessThanOrEqual(layout.clientWidth);
		expectInsideViewport(layout.brand, layout.clientWidth);
		expectInsideViewport(layout.version, layout.clientWidth);
		for (const action of layout.actions) {
			expectInsideViewport(action.rect, layout.clientWidth);
			expect(action.rect.height).toBeGreaterThanOrEqual(44);
		}
		expect(layout.brand?.bottom).toBeLessThanOrEqual(layout.version?.top ?? 0);
		expectNonOverlapping([
			layout.version,
			...layout.actions.map((action) => action.rect),
		]);
	}
});

test("keeps AppShell desktop header controls in one non-overlapping row", async ({
	page,
}) => {
	await page.setViewportSize({ width: 1280, height: 900 });
	await page.goto(storyUrl("components-appshell--default"), {
		waitUntil: "networkidle",
	});

	const layout = await appShellLayout(page);
	expect(layout.header?.height).toBeLessThan(90);
	expect(layout.scrollWidth).toBeLessThanOrEqual(layout.clientWidth);
	expect(layout.bodyScrollWidth).toBeLessThanOrEqual(layout.clientWidth);
	expectInsideViewport(layout.brand, layout.clientWidth);
	expectInsideViewport(layout.command, layout.clientWidth);
	expectInsideViewport(layout.version, layout.clientWidth);
	for (const action of layout.actions) {
		expectInsideViewport(action.rect, layout.clientWidth);
	}
	expectNonOverlapping([
		layout.brand,
		layout.command,
		layout.version,
		...layout.actions.map((action) => action.rect),
	]);
});

test("keeps AppShell overlays inside mobile viewports and restores focus", async ({
	page,
}) => {
	for (const viewport of mobileViewports) {
		await page.setViewportSize(viewport);
		await page.goto(storyUrl("components-appshell--default"), {
			waitUntil: "networkidle",
		});
		const initialWidth = await page.evaluate(
			() => document.documentElement.scrollWidth,
		);

		for (const label of appShellActionLabels) {
			const trigger = page.getByRole("button", { name: label });
			await trigger.click();
			const overlay = page
				.locator("[data-radix-popper-content-wrapper]:visible")
				.last();
			await expect(overlay).toBeVisible();
			const box = await overlay.boundingBox();
			if (!box) throw new Error(`${label} overlay has no bounding box`);
			expect(box.x).toBeGreaterThanOrEqual(0);
			expect(box.x + box.width).toBeLessThanOrEqual(viewport.width);
			expect(
				await page.evaluate(() => document.documentElement.scrollWidth),
			).toBe(initialWidth);
			await page.keyboard.press("Escape");
			await expect(trigger).toBeFocused();
			await expect(
				page.locator("[data-radix-popper-content-wrapper]:visible"),
			).toHaveCount(0);
		}

		const versionTrigger = page.locator("header button.xp-badge");
		await versionTrigger.click();
		const versionOverlay = page
			.locator("[data-radix-popper-content-wrapper]:visible")
			.last();
		await expect(versionOverlay).toBeVisible();
		const versionBox = await versionOverlay.boundingBox();
		if (!versionBox) throw new Error("Version popover has no bounding box");
		expect(versionBox.x).toBeGreaterThanOrEqual(0);
		expect(versionBox.x + versionBox.width).toBeLessThanOrEqual(viewport.width);
		expect(
			await page.evaluate(() => document.documentElement.scrollWidth),
		).toBe(initialWidth);
		await page.keyboard.press("Escape");
		await expect(versionTrigger).toBeFocused();
	}
});

test("keeps the desktop command palette inside the viewport", async ({
	page,
}) => {
	await page.setViewportSize({ width: 1280, height: 900 });
	await page.goto(storyUrl("components-appshell--default"), {
		waitUntil: "networkidle",
	});
	const initialWidth = await page.evaluate(
		() => document.documentElement.scrollWidth,
	);
	const trigger = page.getByRole("button", { name: "Open command palette" });
	await trigger.click();

	const dialog = page.getByRole("dialog");
	await expect(dialog).toBeVisible();
	const box = await dialog.boundingBox();
	if (!box) throw new Error("Command palette has no bounding box");
	expect(box.x).toBeGreaterThanOrEqual(0);
	expect(box.x + box.width).toBeLessThanOrEqual(1280);
	expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBe(
		initialWidth,
	);
	await page.keyboard.press("Escape");
	await expect(dialog).toBeHidden();
	expect(
		await page.evaluate(() =>
			document.activeElement?.closest('[role="dialog"]'),
		),
	).toBeNull();
});
