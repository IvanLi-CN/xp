import { expect, test } from "@playwright/test";

test("demo service monitor form supports every monitor method", async ({
	page,
}) => {
	await page.goto("/demo/login");
	await page.getByRole("button", { name: "Enter demo" }).click();
	await page.goto("/demo/service-monitors");
	await page.getByRole("button", { name: "New monitor" }).click();

	const dialog = page.getByRole("dialog", { name: "New monitor" });
	const method = dialog.getByRole("combobox", { name: "Method" });
	await method.click();
	await expect(page.getByRole("option")).toHaveText([
		"HTTPS",
		"HTTP",
		"PING",
		"TCPING",
	]);

	await page.getByRole("option", { name: "PING", exact: true }).click();
	await expect(
		dialog.getByRole("textbox", { name: "Public host" }),
	).toBeVisible();
	await expect(dialog.getByRole("spinbutton", { name: "Port" })).toHaveCount(0);

	await method.click();
	await page.getByRole("option", { name: "TCPING", exact: true }).click();
	await expect(
		dialog.getByRole("textbox", { name: "Public host" }),
	).toBeVisible();
	await expect(dialog.getByRole("spinbutton", { name: "Port" })).toBeVisible();

	await dialog.getByRole("textbox", { name: "Name" }).fill("Demo mail edge");
	await dialog
		.getByRole("textbox", { name: "Public host" })
		.fill("smtp.example.net");
	await dialog.getByRole("spinbutton", { name: "Port" }).fill("587");
	await dialog.getByRole("button", { name: "Run cluster test" }).click();
	await expect(
		dialog.getByText("3 / 3 observers reached the target"),
	).toBeVisible();

	await dialog
		.getByRole("textbox", { name: "Public host" })
		.fill("mail.example.net");
	await expect(
		dialog.getByText("3 / 3 observers reached the target"),
	).toHaveCount(0);
	await dialog
		.getByRole("textbox", { name: "Public host" })
		.fill("smtp.example.net");
	await dialog.getByRole("button", { name: "Run cluster test" }).click();
	await dialog.getByRole("button", { name: "Add monitor" }).click();

	await expect(page.getByText("TCPING · smtp.example.net:587")).toBeVisible();
});

test("demo monitor creation gives cluster test results the primary workspace", async ({
	page,
}) => {
	await page.goto("/demo/login");
	await page.getByRole("button", { name: "Enter demo" }).click();
	await page.goto("/demo/service-monitors");
	await page.getByRole("button", { name: "New monitor" }).click();

	const dialog = page.getByRole("dialog", { name: "New monitor" });
	await expect(
		dialog.getByRole("heading", { name: "Cluster test results" }),
	).toBeVisible();
	await expect(dialog.getByText("Observer", { exact: true })).toBeVisible();
	await expect(
		dialog.getByText(
			"Target evidence is the primary decision surface before creation.",
		),
	).toBeVisible();
	await expect(dialog.getByText("observer set: all-capable")).toBeVisible();
});
