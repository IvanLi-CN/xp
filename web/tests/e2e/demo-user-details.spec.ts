import { expect, test } from "@playwright/test";
import { fixtureCatalog } from "../../src/fixture-policy/catalog";

const userId = fixtureCatalog.identifier.userTertiary();

test("demo opens the formal user route without a login gate", async ({
	page,
}) => {
	await page.goto(`/demo/users/${userId}?demo=normal&d_profile=valid`);

	await expect(page.getByRole("heading", { name: "佐藤 未来" })).toBeVisible();
	await expect(page.getByRole("button", { name: "Reset token" })).toBeVisible();
	await expect(
		page.getByRole("button", { name: "Reset credentials" }),
	).toBeVisible();
	await expect(page.getByTestId("subscription-format")).toBeVisible();
});

test("demo identity switching stays on the same formal page", async ({
	page,
}) => {
	await page.goto(`/demo/users/${userId}?demo=normal&d_persona=anonymous`);

	await expect(
		page.getByRole("heading", { name: "Admin token required" }),
	).toBeVisible();
	await page.getByRole("button", { name: "Open Demo Inspector" }).click();
	await page.getByLabel("Persona / authentication").selectOption("admin");

	await expect(page.getByRole("heading", { name: "佐藤 未来" })).toBeVisible();
	await expect(page).toHaveURL(/d_persona=admin/);
});

test("demo Mihomo mirror walkthrough is URL-first and simulated", async ({
	page,
}) => {
	await page.goto(`/demo/users/${userId}?demo=normal&d_profile=valid`);

	const format = page.getByTestId("subscription-format");
	await page.getByText("Mihomo", { exact: true }).last().click();
	await page
		.getByRole("checkbox", {
			name: "Use XP mirror for external resources",
		})
		.check();
	await page.getByRole("button", { name: "Fetch" }).click();

	const dialog = page.getByRole("dialog");
	await expect(dialog).toContainText("XP mirror mode");
	await expect(dialog).toContainText("/api/mihomo/resources/demo-");
	await expect(page).toHaveURL(/d_format=mihomo/);
	await expect(page).toHaveURL(/d_mirror=mirror/);
});

test("demo missing Mihomo profile is a structured simulated error", async ({
	page,
}) => {
	await page.goto(
		`/demo/users/${userId}?demo=normal&d_profile=missing&d_format=mihomo&d_mirror=1&d_preview=1`,
	);

	const dialog = page.getByRole("dialog");
	await expect(dialog).toBeVisible();
	await expect(dialog).toContainText("saved Mihomo profile");
});
