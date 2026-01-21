import { expect, test } from "@playwright/test";

import { setAdminToken, setupApiMocks } from "./helpers";

test("renders nodes, endpoints, users, and grant groups pages", async ({
	page,
}) => {
	await setAdminToken(page);
	await setupApiMocks(page);

	await page.goto("/nodes");
	await expect(
		page.getByRole("heading", { name: "Nodes", exact: true }),
	).toBeVisible();

	await page.goto("/endpoints");
	await expect(
		page.getByRole("heading", { name: "Endpoints", exact: true }),
	).toBeVisible();

	await page.goto("/users");
	await expect(
		page.getByRole("heading", { name: "Users", exact: true }),
	).toBeVisible();

	await page.goto("/grant-groups");
	await expect(
		page.getByRole("heading", { name: "Grant groups", exact: true }),
	).toBeVisible();
});
