import { expect, test } from "@playwright/test";

test("renders health card", async ({ page }) => {
	await page.goto("/");
	await expect(page.getByRole("heading", { name: "xp" })).toBeVisible();
	await expect(page.getByText("Backend health")).toBeVisible();
});
