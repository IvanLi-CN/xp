import { expect, test } from "@playwright/test";

import { setupApiMocks } from "./helpers";

test("redirects to login when missing token", async ({ page }) => {
	await setupApiMocks(page);

	await page.goto("/");

	await expect(page).toHaveURL(/\/login$/);
	await expect(
		page.getByRole("heading", { name: "Admin login" }),
	).toBeVisible();
});

test("saves token and reaches dashboard", async ({ page }) => {
	await setupApiMocks(page);

	await page.goto("/login");
	await page.getByLabel("Token").fill("test-token");
	await page.getByRole("button", { name: "Save & Continue" }).click();

	await expect(page).toHaveURL(/\/$/);
	await expect(page.getByRole("heading", { name: "Dashboard" })).toBeVisible();
	await expect(page.getByText("Backend health")).toBeVisible();
});
