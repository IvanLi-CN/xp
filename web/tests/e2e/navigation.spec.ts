import { expect, test } from "@playwright/test";

import { setAdminToken, setupApiMocks } from "./helpers";

test("renders nodes, endpoints, users, and grants pages", async ({ page }) => {
	await setAdminToken(page);
	await setupApiMocks(page);

	await page.goto("/nodes");
	await expect(page.getByRole("heading", { name: "Nodes" })).toBeVisible();

	await page.goto("/endpoints");
	await expect(page.getByRole("heading", { name: "Endpoints" })).toBeVisible();

	await page.goto("/users");
	await expect(page.getByRole("heading", { name: "Users" })).toBeVisible();

	await page.goto("/grants");
	await expect(page.getByRole("heading", { name: "Grants" })).toBeVisible();
});
