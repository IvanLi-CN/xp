import { expect, test } from "@playwright/test";
import { fixtureCatalog } from "../../src/fixture-policy/catalog";

import { setAdminToken, setupApiMocks } from "./helpers";

type UpgradeState = "idle" | "succeeded" | "failed" | "unsupported";

function upgradeStatus(state: UpgradeState) {
	const lifecycle =
		state === "idle"
			? { finished_at: null, started_at: null }
			: {
					finished_at: fixtureCatalog.slotString.s464(),
					started_at: fixtureCatalog.slotString.s82(),
				};
	return {
		support: {
			supported: true,
			reason: null,
			trigger: "systemd",
		},
		status: {
			state,
			target_tag: state === "idle" ? null : "v3.23.2",
			repo: "IvanLi-CN/xp",
			...lifecycle,
			exit_code: state === "succeeded" ? 0 : state === "idle" ? null : 1,
			message: null,
			updated_at: fixtureCatalog.slotString.s464(),
		},
	};
}

const meshStatus = {
	generated_at: fixtureCatalog.slotString.s82(),
	revision: 1,
	local: {
		node_id: fixtureCatalog.slotString.s32(),
		node_name: fixtureCatalog.slotString.s83(),
		cluster_id: fixtureCatalog.slotString.s84(),
		role: "leader",
		leader_api_base_url: fixtureCatalog.slotString.s85(),
		term: 1,
		mesh_proxy_status: "direct",
		mesh_proxy_reason: null,
		canary: {
			enabled: false,
			bind: fixtureCatalog.optional.none(),
			acme_directory_url: null,
			cert_not_after: null,
			last_renewed_at: null,
			last_error: null,
		},
	},
	peers: [],
	events: [],
};

for (const terminalState of ["succeeded", "failed", "unsupported"] as const) {
	test(`keeps the console usable when an upgrade reports ${terminalState}`, async ({
		page,
	}) => {
		await setAdminToken(page);
		await setupApiMocks(page, { mockStatusEvents: false });

		let latestUpgradeStatus = upgradeStatus("idle");
		await page.route("**/api/version/check**", async (route) => {
			await route.fulfill({
				contentType: "application/json",
				body: JSON.stringify({
					current: { package: "3.23.1", release_tag: "v3.23.1" },
					latest: {
						release_tag: "v3.23.2",
						published_at: fixtureCatalog.slotString.s82(),
					},
					has_update: true,
					checked_at: fixtureCatalog.slotString.s82(),
					compare_reason: "update_available",
					source: {
						kind: "github_release",
						repo: "IvanLi-CN/xp",
						api_base: "https://api.github.com",
						channel: "stable",
					},
				}),
			});
		});
		await page.route("**/api/admin/upgrade/status**", async (route) => {
			await route.fulfill({
				contentType: "application/json",
				body: JSON.stringify(latestUpgradeStatus),
			});
		});
		await page.route("**/api/admin/upgrade/start**", async (route) => {
			latestUpgradeStatus = upgradeStatus(terminalState);
			await route.fulfill({
				contentType: "application/json",
				body: JSON.stringify(latestUpgradeStatus),
			});
		});
		await page.route("**/api/admin/mesh/status**", async (route) => {
			await route.fulfill({
				contentType: "application/json",
				body: JSON.stringify(meshStatus),
			});
		});

		const runtimeErrors: string[] = [];
		page.on("console", (message) => {
			if (message.type() === "error") runtimeErrors.push(message.text());
		});
		page.on("pageerror", (error) => runtimeErrors.push(error.message));

		await page.goto("/system-status");
		await expect(
			page.getByRole("heading", { name: "System status", exact: true }),
		).toBeVisible();
		await page
			.getByRole("button", {
				name: "New version v3.23.2 is available.",
			})
			.press("Enter");
		await page.getByRole("button", { name: "Upgrade" }).click();
		await page.getByRole("button", { name: "Start upgrade" }).click();

		const terminalResult =
			terminalState === "succeeded"
				? "last succeeded"
				: terminalState === "failed"
					? "last failed"
					: "ready";
		await expect(page.getByText(terminalResult, { exact: true })).toBeVisible();
		expect(
			await page.evaluate(() => {
				const value = window.sessionStorage.getItem("xp.upgrade-observation");
				return value ? JSON.parse(value).phase : null;
			}),
		).toBe("terminal");
		await expect(
			page.getByRole("heading", { name: "System status", exact: true }),
		).toBeVisible();
		await expect(
			page.getByText("Something went wrong!", { exact: true }),
		).toHaveCount(0);
		await page.waitForTimeout(500);
		expect(
			runtimeErrors.some((message) =>
				/Minified React error #185|Maximum update depth exceeded/i.test(
					message,
				),
			),
		).toBe(false);
	});
}

test("clears an ambiguous start error after status confirms success", async ({
	page,
}) => {
	await setAdminToken(page);
	await setupApiMocks(page, { mockStatusEvents: false });

	let latestUpgradeStatus = upgradeStatus("idle");
	await page.route("**/api/version/check**", async (route) => {
		await route.fulfill({
			contentType: "application/json",
			body: JSON.stringify({
				current: { package: "3.23.1", release_tag: "v3.23.1" },
				latest: {
					release_tag: "v3.23.2",
					published_at: fixtureCatalog.slotString.s82(),
				},
				has_update: true,
				checked_at: fixtureCatalog.slotString.s82(),
				compare_reason: "update_available",
				source: {
					kind: "github_release",
					repo: "IvanLi-CN/xp",
					api_base: "https://api.github.com",
					channel: "stable",
				},
			}),
		});
	});
	await page.route("**/api/admin/upgrade/status**", async (route) => {
		await route.fulfill({
			contentType: "application/json",
			body: JSON.stringify(latestUpgradeStatus),
		});
	});
	await page.route("**/api/admin/upgrade/start**", async (route) => {
		latestUpgradeStatus = upgradeStatus("succeeded");
		await route.fulfill({
			status: 502,
			contentType: "application/json",
			body: JSON.stringify({ message: "restart boundary" }),
		});
	});
	await page.route("**/api/admin/mesh/status**", async (route) => {
		await route.fulfill({
			contentType: "application/json",
			body: JSON.stringify(meshStatus),
		});
	});

	await page.goto("/system-status");
	await expect(
		page.getByRole("heading", { name: "System status", exact: true }),
	).toBeVisible();
	await page
		.getByRole("button", { name: "New version v3.23.2 is available." })
		.press("Enter");
	await page.getByRole("button", { name: "Upgrade" }).click();
	await page.getByRole("button", { name: "Start upgrade" }).click();

	await expect(page.getByText("last succeeded", { exact: true })).toBeVisible();
	await page
		.getByRole("heading", { name: "System status", exact: true })
		.click();
	await page
		.getByRole("button", { name: "Last upgrade completed to v3.23.2." })
		.click();
	await expect(
		page.getByText("502: request failed: 502", { exact: true }),
	).toHaveCount(0);
});
