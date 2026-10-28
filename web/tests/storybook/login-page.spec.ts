import { expect, test } from "@playwright/test";

import { fixtureCatalog } from "../../src/fixture-policy/catalog";

function storyUrl(storyId: string) {
	return `/iframe.html?viewMode=story&id=${storyId}&globals=theme:dark;density:comfortable`;
}

test("exposes the pre-auth service-node selector and known candidates", async ({
	page,
}) => {
	await page.goto(storyUrl("pages-loginpage--pre-auth-backend-recovery"), {
		waitUntil: "networkidle",
	});

	const selector = page.getByRole("button", { name: "Select service node" });
	await expect(selector).toBeVisible();
	await selector.click();
	await expect(page.getByRole("menu")).toBeVisible();
	await expect(
		page.getByRole("menuitem", {
			name: new RegExp(fixtureCatalog.identifier.nodeNamePrimary()),
		}),
	).toBeVisible();
	await page.keyboard.press("Escape");
	await expect(selector).toBeFocused();
});

test("keeps the selector and token form inside a mobile viewport", async ({
	page,
}) => {
	await page.setViewportSize({ width: 393, height: 852 });
	await page.goto(storyUrl("pages-loginpage--zero-candidates"), {
		waitUntil: "networkidle",
	});

	const layout = await page.evaluate(() => {
		const selector = document.querySelector(
			'button[aria-label="Select service node"]',
		);
		const token = document.querySelector('input[name="token"]');
		const rect = (element: Element | null) => {
			if (!element) return null;
			const box = element.getBoundingClientRect();
			return { left: box.left, right: box.right, width: box.width };
		};
		return {
			selector: rect(selector),
			token: rect(token),
			clientWidth: document.documentElement.clientWidth,
			scrollWidth: document.documentElement.scrollWidth,
		};
	});

	expect(layout.scrollWidth).toBeLessThanOrEqual(layout.clientWidth);
	for (const control of [layout.selector, layout.token]) {
		expect(control).not.toBeNull();
		expect(control?.left).toBeGreaterThanOrEqual(0);
		expect(control?.right).toBeLessThanOrEqual(layout.clientWidth);
	}
});
