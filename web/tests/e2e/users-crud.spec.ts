import { expect, test } from "@playwright/test";

import { setAdminToken, setupApiMocks, stubClipboard } from "./helpers";

test("creates and deletes a user, fetches subscription", async ({ page }) => {
	await setAdminToken(page);
	await stubClipboard(page);
	await setupApiMocks(page, { users: [] });

	await page.goto("/users");
	await expect(page.getByText("No users yet")).toBeVisible();

	await page.getByRole("link", { name: "New user" }).click();
	await expect(page.getByRole("heading", { name: "New user" })).toBeVisible();

	await page.getByLabel("Display name").fill("Test User");
	await page.getByRole("button", { name: "Create user" }).click();

	await expect(
		page.getByRole("heading", { name: "User", exact: true }),
	).toBeVisible();

	await page.getByRole("button", { name: "Copy", exact: true }).click();
	await expect(page.getByRole("button", { name: "Copied" })).toBeVisible();

	await expect(page.locator("textarea")).toHaveValue(/vless:\/\//);

	await page.getByRole("button", { name: "Delete user" }).click();
	const dialog = page.locator("dialog[open]");
	await expect(dialog).toBeVisible();
	await dialog.getByRole("button", { name: "Delete" }).click();

	await expect(page).toHaveURL(/\/users$/);
	await expect(page.getByText("No users yet")).toBeVisible();
});
