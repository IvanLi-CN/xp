import { expect, test } from "@playwright/test";

import { fixtureCatalog } from "@/fixture-policy/catalog";
import { setAdminToken, setupApiMocks } from "./helpers";

test.describe("managed VLESS autocomplete", () => {
	test("shows the XP HTTPS listener and access-host suggestions on the create page", async ({
		page,
	}) => {
		await setAdminToken(page);
		await setupApiMocks(page, {
			nodes: [
				{
					node_id: fixtureCatalog.identifier.nodePrimary(),
					node_name: fixtureCatalog.identifier.nodePrimary(),
					api_base_url: fixtureCatalog.url.primaryApi(),
					access_host: fixtureCatalog.host.primary(),
					quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
					quota_reset: fixtureCatalog.quota.reset(),
				},
			],
			endpoints: [
				{
					endpoint_id: fixtureCatalog.identifier.endpointPrimary(),
					node_id: fixtureCatalog.identifier.nodePrimary(),
					tag: fixtureCatalog.identifier.endpointTagPrimary(),
					kind: fixtureCatalog.endpoint.vlessKind(),
					port: fixtureCatalog.endpoint.port443(),
					meta: {
						managed_default: true,
						reality: {
							dest: fixtureCatalog.address.loopback49043(),
							server_names: fixtureCatalog.list.primaryServerNames(),
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
			page.getByRole("button", { name: "Show XP HTTPS listener suggestions" }),
		).toBeVisible();
		await page
			.getByRole("button", { name: "Show XP HTTPS listener suggestions" })
			.click();
		const canarySuggestions = page
			.getByTestId("autocomplete-suggestions")
			.getByRole("option");
		await expect(canarySuggestions.nth(0)).toHaveText(
			"https://127.0.0.1:49043",
		);
		await expect(canarySuggestions.nth(1)).toHaveText(
			"https://127.0.0.1:39043",
		);

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
				.getByText(`${fixtureCatalog.host.primary()}:8443`),
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
					node_id: fixtureCatalog.identifier.nodeSecondary(),
					node_name: fixtureCatalog.identifier.nodeSecondary(),
					api_base_url: fixtureCatalog.url.secondaryApi(),
					access_host: fixtureCatalog.host.secondary(),
					quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
					quota_reset: fixtureCatalog.quota.reset(),
				},
			],
			endpoints: [
				{
					endpoint_id: fixtureCatalog.identifier.endpointSecondary(),
					node_id: fixtureCatalog.identifier.nodeSecondary(),
					tag: fixtureCatalog.identifier.endpointTagSecondary(),
					kind: fixtureCatalog.endpoint.vlessKind(),
					port: fixtureCatalog.endpoint.port53844(),
					meta: {
						managed_default: true,
						reality: {
							dest: fixtureCatalog.address.loopback39043(),
							server_names: fixtureCatalog.list.secondaryServerNames(),
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
				.getByText(fixtureCatalog.host.secondary()),
		).toBeVisible();

		await page.goto(
			`/endpoints/${fixtureCatalog.identifier.endpointSecondary()}`,
		);
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
				.getByText(fixtureCatalog.host.secondary()),
		).toBeVisible();
	});
});
