import { expect, test } from "@playwright/test";

test("history repository capacity guard is visible on the real demo surface", async ({
	page,
}) => {
	await page.goto("/ui-demo/system-status");

	await expect(page.getByText("source journal capacity guard")).toBeVisible();
});
