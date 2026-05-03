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
		page.getByRole("heading", { name: "Demo user", exact: true }),
	).toBeVisible();

	await page.getByRole("button", { name: "Open settings" }).click();
	await page.getByRole("combobox", { name: "Theme" }).click();
	await page.getByRole("option", { name: "dark" }).click();
	await expect(page.locator("html")).toHaveAttribute("data-theme", "xp-dark");

	await page.reload();
	await expect(page.locator("html")).toHaveAttribute("data-theme", "xp-dark");

	const storedTheme = await page.evaluate(() =>
		localStorage.getItem("xp_ui_theme"),
	);
	expect(storedTheme).toBe("dark");
});

test("demo shell exposes theme and density controls", async ({ page }) => {
	await page.goto("/demo/login");
	await page.getByRole("button", { name: "Enter demo" }).click();

	await expect(page.getByRole("link", { name: "xp demo" })).toBeVisible();

	await page.getByRole("button", { name: "Open demo settings" }).click();
	await page
		.getByRole("radiogroup", { name: "Demo theme" })
		.getByRole("radio", { name: "Dark" })
		.click();
	await expect(page.locator("html")).toHaveAttribute("data-theme", "xp-dark");

	await page
		.getByRole("radiogroup", { name: "Demo density" })
		.getByRole("radio", { name: "Compact" })
		.click();
	await expect(page.locator("html")).toHaveAttribute("data-density", "compact");

	const storedPrefs = await page.evaluate(() => ({
		density: localStorage.getItem("xp_ui_density"),
		theme: localStorage.getItem("xp_ui_theme"),
	}));
	expect(storedPrefs).toEqual({ density: "compact", theme: "dark" });
});
