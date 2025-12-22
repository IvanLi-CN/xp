import { expect, test } from "@playwright/test";

test("renders health card", async ({ page }) => {
	await page.addInitScript((storageKey) => {
		window.localStorage.setItem(storageKey, "test-token");
	}, "xp_admin_token");
	await page.goto("/");
	await expect(page.getByRole("heading", { name: "xp" })).toBeVisible();
	await expect(page.getByText("Backend health")).toBeVisible();
});
