import { expect, test } from "@playwright/test";

import { setupApiMocks } from "./helpers";

test("theme preference persists after reload", async ({ page }) => {
	await setupApiMocks(page);

	await page.goto("/login");
	await page.getByLabel("Token").fill("test-token");
	await page.getByRole("button", { name: "Save & Continue" }).click();

	await expect(
		page.getByRole("heading", { name: "Dashboard", exact: true }),
	).toBeVisible();

	await page.getByRole("link", { name: "Users" }).click();
	await expect(
		page.getByRole("heading", { name: "Users", exact: true }),
	).toBeVisible();

	await page.getByRole("link", { name: "user-1" }).click();
	await expect(
		page.getByRole("heading", { name: "User", exact: true }),
	).toBeVisible();

	await page.getByRole("button", { name: "Theme" }).click();
	await page.getByLabel("Theme").selectOption("dark");
	await expect(page.locator("html")).toHaveAttribute("data-theme", "xp-dark");

	await page.reload();
	await expect(page.locator("html")).toHaveAttribute("data-theme", "xp-dark");

	const storedTheme = await page.evaluate(() =>
		localStorage.getItem("xp_ui_theme"),
	);
	expect(storedTheme).toBe("dark");
});
