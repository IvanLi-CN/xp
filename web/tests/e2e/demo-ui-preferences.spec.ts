import { expect, test } from "@playwright/test";

test("demo shell exposes theme and density controls", async ({ page }) => {
	await page.goto("/demo/?demo=normal");

	await expect(
		page.getByRole("button", { name: "Open Demo Inspector" }),
	).toBeVisible();

	await page.getByRole("button", { name: "Open Demo Inspector" }).click();
	await page.getByLabel("Network").selectOption("slow");
	await expect(page).toHaveURL(/d_network=slow/);
});
