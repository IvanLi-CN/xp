import { expect, test } from "@playwright/test";
import { fixtureCatalog } from "../../src/fixture-policy/catalog";
import { setAdminToken, setupApiMocks } from "./helpers";

test("creates and deletes a user, fetches subscription", async ({ page }) => {
	await setAdminToken(page);
	await setupApiMocks(page, { users: [] });

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
	const previewFormat = rawDialog.getByTestId("subscription-preview-format");
	await expect(previewFormat.locator('input[type="radio"]')).toHaveCount(3);
	await expect(previewFormat.locator('input[value="raw"]')).toBeChecked();
	await expect(rawDialog.getByLabel("Search")).toHaveCount(0);
	await expect(rawDialog.getByTestId("subscription-code-scroll")).toContainText(
		fixtureCatalog.subscription.rawUri(),
	);
	await previewFormat.locator("label").nth(1).click();
	await expect(previewFormat.locator('input[value="clash"]')).toBeChecked();
	await expect(rawDialog.getByTestId("subscription-code-scroll")).toContainText(
		"reality-opts:",
	);
	await rawDialog.locator("[data-sub-preview-close]").click();

	await page.getByTestId("subscription-format").locator("label").nth(1).click();
	await page.getByTestId("subscription-fetch").click();
	const clashDialog = page.getByRole("dialog");
	await expect(clashDialog).toBeVisible();
	await expect(
		clashDialog.getByTestId("subscription-code-scroll"),
	).toContainText("reality-opts:");
	await expect(
		clashDialog.getByTestId("subscription-code-scroll"),
	).toContainText(fixtureCatalog.endpoint.realityKeys().public_key);
	await clashDialog.locator("[data-sub-preview-close]").click();

	await page.getByRole("button", { name: "Delete user" }).click();
	const confirm = page.getByRole("alertdialog");
	await expect(confirm).toBeVisible();
	await confirm.getByRole("button", { name: "Delete" }).click();

	await expect(page).toHaveURL(/\/users$/);
	await expect(page.getByText("No users yet")).toBeVisible();
});
