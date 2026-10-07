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

test("opens the Mihomo workspace from User Details", async ({ page }) => {
	await setAdminToken(page);
	await setupApiMocks(page);

	await page.goto(`/users/${fixtureCatalog.identifier.userPrimary()}`);
	await page.getByRole("button", { name: "Expand editor" }).click();

	const workspace = page.getByRole("dialog");
	await expect(workspace).toBeVisible();
	await expect(workspace.locator("aside")).toBeVisible();
	await expect(workspace.locator(".cm-editor")).toHaveCount(3);
	await workspace
		.getByRole("button", { name: /extra_proxy_providers_yaml/ })
		.click();
	await expect(workspace).toContainText("extra_proxy_providers_yaml");

	await workspace.getByRole("button", { name: "Exit expanded editor" }).click();
	await expect(workspace).toBeHidden();
});

test("protects production Mihomo drafts before leaving or deleting a user", async ({
	page,
}) => {
	await setAdminToken(page);
	await setupApiMocks(page);

	await page.goto(`/users/${fixtureCatalog.identifier.userPrimary()}`);
	const editor = page.locator(
		'[data-mihomo-document="mixin_yaml"] .cm-content',
	);
	await editor.click();
	await page.keyboard.press("ControlOrMeta+End");
	await page.keyboard.insertText("\n# production dirty navigation");

	await page.getByRole("link", { name: "Back to users" }).click();
	const guard = page.getByRole("alertdialog");
	await expect(
		guard.getByRole("heading", {
			name: "Unsaved Mihomo profile changes",
		}),
	).toBeVisible();
	await guard.getByRole("button", { name: "Keep editing" }).click();
	await expect(editor).toContainText("production dirty navigation");
	await page.getByRole("link", { name: "Dashboard", exact: true }).click();
	await expect(
		page
			.getByRole("alertdialog")
			.getByRole("heading", { name: "Unsaved Mihomo profile changes" }),
	).toBeVisible();
	await page
		.getByRole("alertdialog")
		.getByRole("button", { name: "Keep editing" })
		.click();

	await page.getByRole("button", { name: "Delete user" }).click();
	await page
		.getByRole("alertdialog")
		.getByRole("button", { name: "Delete" })
		.click();
	await page
		.getByRole("alertdialog")
		.getByRole("button", { name: "Discard and continue" })
		.click();
	await expect(page).toHaveURL(/\/users$/);
});

test("protects production Mihomo drafts on browser back", async ({ page }) => {
	await setAdminToken(page);
	await setupApiMocks(page);

	await page.goto("/users");
	await page.getByRole("link", { name: "Demo user", exact: true }).click();
	await expect(
		page.getByRole("heading", { name: "Demo user", exact: true }),
	).toBeVisible();
	const editor = page.locator(
		'[data-mihomo-document="mixin_yaml"] .cm-content',
	);
	await editor.click();
	await page.keyboard.press("ControlOrMeta+End");
	await page.keyboard.insertText("\n# browser back protection");

	await page.goBack();
	const guard = page.getByRole("alertdialog");
	await expect(
		guard.getByRole("heading", {
			name: "Unsaved Mihomo profile changes",
		}),
	).toBeVisible();
	await guard.getByRole("button", { name: "Keep editing" }).click();
	await expect(page).toHaveURL(
		new RegExp(`/users/${fixtureCatalog.identifier.userPrimary()}$`),
	);
	await expect(editor).toContainText("browser back protection");
});

test("persists all three Mihomo documents through the production save path", async ({
	page,
}) => {
	await setAdminToken(page);
	await setupApiMocks(page);

	await page.goto(`/users/${fixtureCatalog.identifier.userPrimary()}`);
	const values = {
		mixin_yaml: "port: 7890\n",
		extra_proxies_yaml: "- name: e2e-proxy\n  type: ss\n",
		extra_proxy_providers_yaml: "ProviderA:\n  type: http\n",
	};
	for (const [documentId, value] of Object.entries(values)) {
		const editor = page.locator(
			`[data-mihomo-document="${documentId}"] .cm-content`,
		);
		await editor.click();
		await page.keyboard.press("ControlOrMeta+A");
		await page.keyboard.insertText(value);
	}
	const saveButton = page
		.getByRole("button", { name: "Save configuration" })
		.first();
	await saveButton.click();
	await expect(saveButton).toBeDisabled();
	await expect(
		page.locator('[data-mihomo-document="mixin_yaml"] .cm-content'),
	).toContainText("port: 7890");
	await expect(page.getByText("Mihomo profile updated")).toBeVisible();

	await page.getByRole("button", { name: "Expand editor" }).click();
	const workspace = page.getByRole("dialog");
	for (const [documentId, value] of Object.entries(values)) {
		await workspace
			.getByRole("button", { name: new RegExp(documentId) })
			.click();
		await expect(
			workspace.locator(`[data-mihomo-document="${documentId}"] .cm-content`),
		).toContainText(value.trim().split("\n")[0]);
	}

	await page.reload();
	for (const [documentId, value] of Object.entries(values)) {
		await expect(
			page.locator(`[data-mihomo-document="${documentId}"] .cm-content`),
		).toContainText(value.trim().split("\n")[0]);
	}
});

test("repairs legacy Mihomo provider fields after API rejection", async ({
	page,
}) => {
	await setAdminToken(page);
	await setupApiMocks(page, {
		mihomoProfile: {
			mixin_yaml:
				"port: 7890\nproxy-providers:\n  LegacyProvider:\n    type: http\n",
			extra_proxies_yaml: "",
			extra_proxy_providers_yaml: "ExistingProvider:\n  type: file\n",
		},
	});

	await page.goto(`/users/${fixtureCatalog.identifier.userPrimary()}`);
	const mixinEditor = page.locator(
		'[data-mihomo-document="mixin_yaml"] .cm-content',
	);
	await mixinEditor.click();
	await page.keyboard.press("ControlOrMeta+End");
	await page.keyboard.insertText("\n# preserve legacy profile");

	const saveButton = page
		.getByRole("button", { name: "Save configuration" })
		.first();
	await saveButton.click();
	await expect(page.getByText("Mihomo profile updated")).toBeVisible();
	await expect(mixinEditor).not.toContainText("proxy-providers:");
	await expect(
		page.locator(
			'[data-mihomo-document="extra_proxy_providers_yaml"] .cm-content',
		),
	).toContainText("LegacyProvider:");

	await page.reload();
	await expect(
		page.locator('[data-mihomo-document="mixin_yaml"] .cm-content'),
	).not.toContainText("proxy-providers:");
	await expect(
		page.locator(
			'[data-mihomo-document="extra_proxy_providers_yaml"] .cm-content',
		),
	).toContainText("LegacyProvider:");
});
