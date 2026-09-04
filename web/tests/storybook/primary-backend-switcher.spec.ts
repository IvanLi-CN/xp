import { expect, test } from "@playwright/test";

function storyUrl(storyId: string) {
	return `/iframe.html?viewMode=story&id=${storyId}&globals=theme:dark;density:comfortable`;
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
		page.getByRole("menuitem", { name: /Recovery node/ }),
	).toBeVisible();
	await expect(
		page.getByRole("menuitem", { name: /Current page/ }),
	).toBeDisabled();
});

test("keeps the switcher menu inside a narrow viewport", async ({ page }) => {
	await page.setViewportSize({ width: 393, height: 852 });
	await page.goto(storyUrl("components-primarybackendswitcher--default"), {
		waitUntil: "networkidle",
	});
	await page.getByRole("button", { name: "Open primary backend" }).click();

	const menu = page.getByRole("menu");
	await expect(menu).toBeVisible();
	const box = await menu.boundingBox();
	if (!box) throw new Error("Primary backend menu has no bounding box");
	expect(box.x).toBeGreaterThanOrEqual(0);
	expect(box.x + box.width).toBeLessThanOrEqual(393);
});

test("keeps AppShell header controls separated on mobile", async ({ page }) => {
	await page.setViewportSize({ width: 393, height: 852 });
	await page.goto(storyUrl("components-appshell--default"), {
		waitUntil: "networkidle",
	});

	const controls = await page.evaluate(() =>
		["Open menu", "Open primary backend", "Open status", "Open settings"].map(
			(label) => {
				const element = document.querySelector<HTMLButtonElement>(
					`button[aria-label="${label}"]`,
				);
				if (!element) throw new Error(`Missing ${label}`);
				const rect = element.getBoundingClientRect();
				return { label, left: rect.left, right: rect.right };
			},
		),
	);
	for (let index = 1; index < controls.length; index += 1) {
		expect(controls[index]?.left).toBeGreaterThanOrEqual(
			controls[index - 1]?.right ?? 0,
		);
	}
});
