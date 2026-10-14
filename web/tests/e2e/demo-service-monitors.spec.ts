import { expect, test } from "@playwright/test";

test("demo service monitors use the formal page and simulated data", async ({
	page,
}) => {
	await page.goto("/demo/monitors?demo=normal");

	await expect(
		page.getByRole("heading", { name: "Service monitoring" }),
	).toBeVisible();
	await expect(page.getByRole("link", { name: "New monitor" })).toBeVisible();
});

test("demo incident state exposes the formal monitor error surface", async ({
	page,
}) => {
	await page.goto("/demo/monitors?demo=incident&d_network=error");

	await expect(
		page.getByRole("heading", { name: "Service monitoring unavailable" }),
	).toBeVisible();
	await expect(
		page.getByText(
			"The capabilities endpoint returned an invalid or failed response",
		),
	).toBeVisible();
});
