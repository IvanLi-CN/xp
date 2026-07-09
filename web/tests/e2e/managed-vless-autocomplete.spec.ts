import { expect, test } from "@playwright/test";

import { setAdminToken, setupApiMocks } from "./helpers";

test.describe("managed VLESS autocomplete", () => {
	test("shows the XP HTTPS listener and access-host suggestions on the create page", async ({
		page,
	}) => {
		await setAdminToken(page);
		await setupApiMocks(page, {
			nodes: [
				{
					node_id: "node-alpha",
					node_name: "alpha",
					api_base_url: "https://node-xp.example.test:443",
					access_host: "node-xp.example.test",
					quota_limit_bytes: 0,
					quota_reset: {
						policy: "monthly",
						day_of_month: 1,
						tz_offset_minutes: null,
					},
				},
			],
			endpoints: [],
		});

		await page.goto("/endpoints/new");
		await expect(
			page.getByRole("heading", { name: "New endpoint", exact: true }),
		).toBeVisible();

		await expect(
			page.getByRole("button", { name: "Show XP HTTPS listener suggestions" }),
		).toBeVisible();
		await page
			.getByRole("button", { name: "Show XP HTTPS listener suggestions" })
			.click();
		await expect(page.getByText("https://127.0.0.1:39043")).toBeVisible();

		await expect(
			page.getByRole("button", { name: "Show access host suggestions" }),
		).toBeVisible();
		await page
			.getByRole("spinbutton", { name: "port", exact: true })
			.fill("8443");
		await page
			.getByRole("button", { name: "Show access host suggestions" })
			.click();
		await expect(
			page
				.getByTestId("tag-input-suggestions")
				.getByText("node-xp.example.test:8443"),
		).toBeVisible();
	});

	test([
		"keeps the XP HTTPS listener suggestion visible",
		"without history on create and update pages",
	].join(" "), async ({ page }) => {
		await setAdminToken(page);
		await setupApiMocks(page, {
			nodes: [
				{
					node_id: "node-hinet",
					node_name: "hinet",
					api_base_url: "https://hinet-xp.707979.xyz",
					access_host: "hinet-ep.707979.xyz",
					quota_limit_bytes: 0,
					quota_reset: {
						policy: "monthly",
						day_of_month: 1,
						tz_offset_minutes: null,
					},
				},
			],
			endpoints: [
				{
					endpoint_id: "endpoint-hinet-managed",
					node_id: "node-hinet",
					tag: "managed-hinet",
					kind: "vless_reality_vision_tcp",
					port: 53844,
					meta: {
						managed_default: true,
						reality: {
							dest: "127.0.0.1:39043",
							server_names: ["hinet-ep.707979.xyz"],
							server_names_source: "manual",
							fingerprint: "chrome",
						},
					},
				},
			],
		});

		await page.goto("/endpoints/new");
		await expect(
			page.getByRole("heading", { name: "New endpoint", exact: true }),
		).toBeVisible();
		await expect(
			page.getByRole("button", {
				name: "Show XP HTTPS listener suggestions",
			}),
		).toBeVisible();
		await page
			.getByRole("button", {
				name: "Show XP HTTPS listener suggestions",
			})
			.click();
		await expect(
			page
				.getByTestId("autocomplete-suggestions")
				.getByText("https://127.0.0.1:39043"),
		).toBeVisible();
		await expect(
			page.getByRole("button", { name: "Show access host suggestions" }),
		).toBeVisible();
		await page
			.getByRole("button", { name: "Show access host suggestions" })
			.click();
		await expect(
			page
				.getByTestId("tag-input-suggestions")
				.getByText("hinet-ep.707979.xyz"),
		).toBeVisible();

		await page.goto("/endpoints/endpoint-hinet-managed");
		await expect(
			page.getByRole("heading", { name: "Endpoint details", exact: true }),
		).toBeVisible();
		await expect(
			page.getByRole("button", {
				name: "Show XP HTTPS listener suggestions",
			}),
		).toBeVisible();
		await page
			.getByRole("button", {
				name: "Show XP HTTPS listener suggestions",
			})
			.click();
		await expect(
			page
				.getByTestId("autocomplete-suggestions")
				.getByText("https://127.0.0.1:39043"),
		).toBeVisible();
		await expect(
			page.getByRole("button", { name: "Show access host suggestions" }),
		).toBeVisible();
		await page
			.getByRole("button", { name: "Show access host suggestions" })
			.click();
		await expect(
			page
				.getByTestId("tag-input-suggestions")
				.getByText("hinet-ep.707979.xyz"),
		).toBeVisible();
	});
});
