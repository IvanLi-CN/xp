import { expect, test } from "@playwright/test";

test("demo user details follow the production user-management layout", async ({
	page,
}) => {
	await page.goto("/demo/login");
	await page.getByRole("button", { name: "Enter demo" }).click();
	await page.goto("/demo/users/user-sato");

	await expect(
		page.getByRole("heading", { name: "佐藤 未来", exact: true }),
	).toBeVisible();
	await expect(page.getByRole("button", { name: "Reset token" })).toBeVisible();
	await expect(
		page.getByRole("button", { name: "Reset credentials" }),
	).toBeVisible();
	await expect(page.getByRole("button", { name: "Delete user" })).toBeVisible();

	await expect(page.getByText("Display name")).toBeVisible();
	await expect(page.getByText("Subscription token:")).toBeVisible();
	await expect(page.getByText("Mihomo mixin config")).toBeVisible();
	await page.getByTestId("demo-subscription-format").click();
	await expect(page.getByRole("option", { name: "raw" })).toBeVisible();
	await expect(page.getByRole("option", { name: "clash" })).toBeVisible();
	await expect(
		page.getByRole("option", { name: "mihomo(provider)" }),
	).toBeVisible();
	await expect(page.getByRole("option", { name: /legacy/i })).toHaveCount(0);
	await expect(page.getByRole("option", { name: /default/i })).toHaveCount(0);
	await page.keyboard.press("Escape");

	await page.getByRole("button", { name: "Access" }).click();
	await expect(
		page.getByRole("button", { name: "Apply access" }),
	).toBeVisible();
	await expect(page.getByText("Selected endpoints:")).toBeVisible();
	await expect(page.getByRole("table")).toContainText("VLESS");

	await page.getByRole("button", { name: "Quota status" }).click();
	await expect(page.getByText("node-tokyo-1")).toBeVisible();
	await expect(page.getByText("node-sgp-1")).toBeVisible();

	await page.getByRole("button", { name: "Usage details" }).click();
	await expect(page.getByText(/Usage details ·/)).toBeVisible();
	await expect(page.getByRole("table")).toContainText("Inbound IPs");

	await page.getByRole("button", { name: "User", exact: true }).click();
	await page.getByRole("button", { name: "Fetch" }).click();
	const dialog = page.getByRole("dialog");
	await expect(dialog).toBeVisible();
	await expect(dialog.getByText("Subscription preview")).toBeVisible();
	await expect(dialog).toContainText("vless://");
});

test("demo service config reflects provider-only mihomo delivery", async ({
	page,
}) => {
	await page.goto("/demo/login");
	await page.getByRole("button", { name: "Enter demo" }).click();
	await page.goto("/demo/service-config");

	await expect(
		page.getByRole("heading", { name: "Service config" }),
	).toBeVisible();
	await expect(page.getByText("provider-only")).toHaveCount(2);
	await expect(page.getByText("format=mihomo")).toHaveCount(2);
	await expect(page.getByText("Default subscription format")).toHaveCount(0);
	await expect(page.getByText("Mihomo default delivery")).toHaveCount(0);
	await expect(page.getByText("Inline proxies")).toHaveCount(0);
});
