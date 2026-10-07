import { expect, test } from "@playwright/test";
import { fixtureCatalog } from "../../src/fixture-policy/catalog";

test("demo user details follow the production user-management layout", async ({
	page,
}) => {
	await page.goto("/demo/login");
	await page.getByRole("button", { name: "Enter demo" }).click();
	await page.goto(`/demo/users/${fixtureCatalog.identifier.userTertiary()}`);

	await expect(
		page.getByRole("heading", { name: "佐藤 未来", exact: true }),
	).toBeVisible();
	await expect(page.getByRole("button", { name: "Reset token" })).toBeVisible();
	await expect(
		page.getByRole("button", { name: "Reset credentials" }),
	).toBeVisible();
	await expect(page.getByRole("button", { name: "Delete user" })).toBeVisible();

	await expect(page.getByText("Display name", { exact: true })).toBeVisible();
	await expect(page.getByText("Subscription token:")).toBeVisible();
	await expect(page.getByText("Mihomo mixin config")).toBeVisible();
	const subscriptionFormat = page.getByTestId("demo-subscription-format");
	await expect(subscriptionFormat.locator('input[type="radio"]')).toHaveCount(
		3,
	);
	await expect(subscriptionFormat.locator('input[value="raw"]')).toBeChecked();
	await expect(subscriptionFormat.locator('input[value="clash"]')).toHaveCount(
		1,
	);
	await expect(subscriptionFormat.locator('input[value="mihomo"]')).toHaveCount(
		1,
	);
	await subscriptionFormat.locator("label").nth(2).click();
	await expect(
		subscriptionFormat.locator('input[value="mihomo"]'),
	).toBeChecked();
	await subscriptionFormat.locator("label").nth(0).click();
	await expect(subscriptionFormat.locator('input[value="raw"]')).toBeChecked();

	await page.getByRole("button", { name: "Access" }).click();
	await expect(
		page.getByRole("button", { name: "Apply access" }),
	).toBeVisible();
	await expect(page.getByText("Selected endpoints:")).toBeVisible();
	await expect(page.getByRole("table")).toContainText("VLESS");

	await page.getByRole("button", { name: "Quota status" }).click();
	await expect(
		page.getByText(fixtureCatalog.identifier.nodePrimary()),
	).toBeVisible();
	await expect(
		page.getByText(fixtureCatalog.identifier.nodeTertiary()),
	).toBeVisible();

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

test("expands the Mihomo editor into a persistent file workspace", async ({
	page,
}) => {
	await page.goto("/demo/login");
	await page.getByRole("button", { name: "Enter demo" }).click();
	await page.goto(`/demo/users/${fixtureCatalog.identifier.userTertiary()}`);

	const pageUrl = page.url();
	await page.getByRole("button", { name: "Expand editor" }).click();
	const workspace = page.getByRole("dialog");
	await expect(workspace).toBeVisible();
	await expect(workspace.locator("aside")).toBeVisible();
	await expect(workspace.locator(".cm-editor")).toHaveCount(3);

	const mixinEditor = workspace.locator(
		'[data-mihomo-document="mixin_yaml"] .cm-content',
	);
	await mixinEditor.click();
	await page.keyboard.press("ControlOrMeta+End");
	await page.keyboard.insertText("\n# workspace edit");
	await workspace.getByRole("button", { name: /extra_proxies_yaml/ }).click();
	await workspace.getByRole("button", { name: /mixin_yaml/ }).click();
	await expect(mixinEditor).toContainText("workspace edit");

	await workspace.getByRole("button", { name: "Exit expanded editor" }).click();
	await expect(workspace).toBeHidden();
	await expect(page).toHaveURL(pageUrl);
	await expect(
		page.getByRole("button", { name: "Expand editor" }),
	).toBeVisible();

	await page.getByRole("button", { name: "Access" }).click();
	await page.getByRole("button", { name: "User", exact: true }).click();
	await page.getByRole("button", { name: "Expand editor" }).click();
	const reopenedWorkspace = page.getByRole("dialog");
	await expect(
		reopenedWorkspace.locator(
			'[data-mihomo-document="mixin_yaml"] .cm-content',
		),
	).toContainText("workspace edit");
	await reopenedWorkspace
		.getByRole("button", { name: "Exit expanded editor" })
		.click();
});

test("uses the Files drawer at a narrow viewport", async ({ page }) => {
	await page.setViewportSize({ width: 393, height: 852 });
	await page.goto("/demo/login");
	await page.getByRole("button", { name: "Enter demo" }).click();
	await page.goto(`/demo/users/${fixtureCatalog.identifier.userTertiary()}`);

	await page.getByRole("button", { name: "Expand editor" }).click();
	const workspace = page.getByRole("dialog").first();
	await expect(workspace.locator("aside")).toBeHidden();
	await workspace.getByRole("button", { name: "Files" }).click();
	const files = page.getByRole("heading", { name: "Files" }).last();
	await expect(files).toBeVisible();
	await page
		.getByRole("button", { name: /extra_proxy_providers_yaml/ })
		.last()
		.click();
	await expect(files).toBeHidden();
	await expect(workspace).toContainText("extra_proxy_providers_yaml");
});

for (const viewport of [
	{ width: 320, height: 852 },
	{ width: 360, height: 800 },
	{ width: 393, height: 852 },
]) {
	test(`keeps the mobile YAML editor and delete dialog within ${viewport.width}px`, async ({
		page,
	}) => {
		await page.setViewportSize(viewport);
		await page.goto("/demo/login");
		await page.getByRole("button", { name: "Enter demo" }).click();
		await page.goto(`/demo/users/${fixtureCatalog.identifier.userTertiary()}`);

		const editor = page.locator(".cm-content").first();
		await editor.click();
		await page.keyboard.press("ControlOrMeta+A");
		await page.keyboard.insertText(`proxy: ${"x".repeat(1200)}`);

		const layout = await page.evaluate(() => ({
			clientWidth: document.documentElement.clientWidth,
			documentScrollWidth: document.documentElement.scrollWidth,
			bodyScrollWidth: document.body.scrollWidth,
			editors: [...document.querySelectorAll(".cm-editor")].map((node) => {
				const rect = node.getBoundingClientRect();
				return { left: rect.left, right: rect.right };
			}),
			scrollers: [...document.querySelectorAll(".cm-scroller")].map((node) => ({
				clientWidth: node.clientWidth,
				scrollWidth: node.scrollWidth,
			})),
		}));

		expect(layout.documentScrollWidth).toBeLessThanOrEqual(layout.clientWidth);
		expect(layout.bodyScrollWidth).toBeLessThanOrEqual(layout.clientWidth);
		expect(
			layout.editors.every(
				(editorBounds) =>
					editorBounds.left >= 0 &&
					editorBounds.right <= layout.clientWidth + 1,
			),
		).toBe(true);
		expect(layout.scrollers[0]?.scrollWidth).toBeGreaterThan(
			layout.scrollers[0]?.clientWidth ?? 0,
		);

		await page
			.getByRole("button", { name: "Delete user", exact: true })
			.click();
		const dialog = page.getByRole("alertdialog", { name: "Delete user" });
		await expect(dialog).toBeVisible();
		const dialogBounds = await dialog.evaluate((node) => {
			const rect = node.getBoundingClientRect();
			return {
				left: rect.left,
				right: rect.right,
				top: rect.top,
				bottom: rect.bottom,
				visualWidth: window.visualViewport?.width ?? window.innerWidth,
				visualHeight: window.visualViewport?.height ?? window.innerHeight,
			};
		});
		expect(dialogBounds.left).toBeGreaterThanOrEqual(0);
		expect(dialogBounds.right).toBeLessThanOrEqual(dialogBounds.visualWidth);
		expect(dialogBounds.top).toBeGreaterThanOrEqual(0);
		expect(dialogBounds.bottom).toBeLessThanOrEqual(dialogBounds.visualHeight);
		await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
		await expect(dialog).toBeHidden();
	});
}

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
