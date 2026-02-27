import { expect, test } from "@playwright/test";

import { setAdminToken, setupApiMocks } from "./helpers";

test("creates and deletes a user, fetches subscription", async ({ page }) => {
	await setAdminToken(page);
	await setupApiMocks(page, {
		users: [],
		subscriptionContentRaw:
			"vless://example-host?encryption=none\nvless://second-host?encryption=none\n",
		subscriptionContentClash: `proxies:
  - name: demo
    type: vless
    servername: example.com
    reality-opts:
      public-key: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
      short-id: 0123456789abcdef
`,
	});

	await page.goto("/users");
	await expect(page.getByText("No users yet")).toBeVisible();

	await page.getByRole("link", { name: "New user" }).click();
	await expect(page.getByRole("heading", { name: "New user" })).toBeVisible();

	await page.getByLabel("Display name").fill("Test User");
	await page.getByRole("button", { name: "Create user" }).click();

	await expect(
		page.getByRole("heading", { name: "Test User", exact: true }),
	).toBeVisible();

	await page.getByTestId("subscription-fetch").click();
	const rawDialog = page.getByRole("dialog");
	await expect(rawDialog).toBeVisible();
	await expect(rawDialog.getByText("Subscription preview")).toBeVisible();
	await expect(rawDialog.getByTestId("subscription-code-scroll")).toContainText(
		"vless://example-host?encryption=none",
	);
	await rawDialog.locator("[data-sub-preview-close]").click();

	await page.getByTestId("subscription-format").selectOption("clash");
	await page.getByTestId("subscription-fetch").click();
	const clashDialog = page.getByRole("dialog");
	await expect(clashDialog).toBeVisible();
	await expect(
		clashDialog.getByTestId("subscription-code-scroll"),
	).toContainText("reality-opts:");
	await expect(
		clashDialog.getByTestId("subscription-code-scroll"),
	).toContainText(
		"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
	);
	await clashDialog.locator("[data-sub-preview-close]").click();

	await page.getByRole("button", { name: "Delete user" }).click();
	const confirm = page.locator("dialog[open]");
	await expect(confirm).toBeVisible();
	await confirm.getByRole("button", { name: "Delete" }).click();

	await expect(page).toHaveURL(/\/users$/);
	await expect(page.getByText("No users yet")).toBeVisible();
});
