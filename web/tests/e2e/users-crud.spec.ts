import { expect, test } from "@playwright/test";

import { setAdminToken, setupApiMocks, stubClipboard } from "./helpers";

test("creates and deletes a user, fetches subscription", async ({ page }) => {
	await setAdminToken(page);
	await stubClipboard(page);
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
		page.getByRole("heading", { name: "User", exact: true }),
	).toBeVisible();

	await expect(page.locator("textarea")).toHaveCount(0);

	await page.getByTestId("subscription-format").selectOption("raw");
	await page.getByTestId("subscription-fetch").click();

	const dialog = page.locator("dialog[open]");
	await expect(dialog).toBeVisible();
	await expect(
		dialog.getByRole("heading", { name: "Subscription preview" }),
	).toBeVisible();

	const codeScroll = dialog.getByTestId("subscription-code-scroll");
	await expect(codeScroll).toBeVisible();
	await expect(codeScroll).toContainText("vless://example-host");

	const firstLine = codeScroll.locator('[data-line="0"]');
	const whiteSpace = await firstLine.evaluate(
		(el) => getComputedStyle(el).whiteSpace,
	);
	expect(whiteSpace).toBe("pre");

	await dialog.getByRole("button", { name: "Copy content" }).click();
	const rawCopied = await page.evaluate(() => {
		// @ts-expect-error -- test-only helper
		return window.__xp_clipboard_last_write as string;
	});
	expect(rawCopied).toBe(
		"vless://example-host?encryption=none\nvless://second-host?encryption=none\n",
	);

	await dialog.getByRole("button", { name: "Close", exact: true }).click();
	await expect(page.locator("dialog[open]")).toHaveCount(0);

	await page.getByTestId("subscription-format").selectOption("clash");
	await page.getByTestId("subscription-fetch").click();
	const dialog2 = page.locator("dialog[open]");
	await expect(dialog2).toBeVisible();
	await expect(dialog2.getByTestId("subscription-code-scroll")).toContainText(
		"reality-opts:",
	);

	await dialog2
		.getByText("public-key")
		.locator("..")
		.getByRole("button", { name: "Copy" })
		.click();
	const publicKeyCopied = await page.evaluate(() => {
		// @ts-expect-error -- test-only helper
		return window.__xp_clipboard_last_write as string;
	});
	expect(publicKeyCopied).toBe(
		"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
	);

	await dialog2.getByRole("button", { name: "Close", exact: true }).click();
	await expect(page.locator("dialog[open]")).toHaveCount(0);

	await page.getByRole("button", { name: "Delete user" }).click();
	const confirm = page.locator("dialog[open]");
	await expect(confirm).toBeVisible();
	await confirm.getByRole("button", { name: "Delete" }).click();

	await expect(page).toHaveURL(/\/users$/);
	await expect(page.getByText("No users yet")).toBeVisible();
});
