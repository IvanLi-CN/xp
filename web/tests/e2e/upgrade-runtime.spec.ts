import { expect, test } from "@playwright/test";

import { setAdminToken, setupApiMocks } from "./helpers";

type UpgradeState = "idle" | "succeeded" | "failed" | "unsupported";

function upgradeStatus(state: UpgradeState) {
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
			started_at: state === "idle" ? null : "2026-08-07T00:00:00Z",
			finished_at: state === "idle" ? null : "2026-08-07T00:00:01Z",
			exit_code: state === "succeeded" ? 0 : state === "idle" ? null : 1,
			message: null,
			updated_at: "2026-08-07T00:00:01Z",
		},
	};
}

const meshStatus = {
	generated_at: "2026-08-07T00:00:00Z",
	revision: 1,
	local: {
		node_id: "node-1",
		node_name: "Node 1",
		cluster_id: "cluster-1",
		role: "leader",
		leader_api_base_url: "https://xp.example.test",
		term: 1,
		mesh_proxy_status: "direct",
		mesh_proxy_reason: null,
		canary: {
			enabled: false,
			bind: null,
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

		await page.route("**/api/version/check", async (route) => {
			await route.fulfill({
				contentType: "application/json",
				body: JSON.stringify({
					current: { package: "3.23.1", release_tag: "v3.23.1" },
					latest: {
						release_tag: "v3.23.2",
						published_at: "2026-08-07T00:00:00Z",
					},
					has_update: true,
					checked_at: "2026-08-07T00:00:00Z",
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
		await page.route("**/api/admin/upgrade/status", async (route) => {
			await route.fulfill({
				contentType: "application/json",
				body: JSON.stringify(upgradeStatus("idle")),
			});
		});
		await page.route("**/api/admin/upgrade/start", async (route) => {
			await route.fulfill({
				contentType: "application/json",
				body: JSON.stringify(upgradeStatus(terminalState)),
			});
		});
		await page.route("**/api/admin/mesh/status", async (route) => {
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
			.click();
		await page.getByRole("button", { name: "Upgrade" }).click();
		await page.getByRole("button", { name: "Start upgrade" }).click();

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
