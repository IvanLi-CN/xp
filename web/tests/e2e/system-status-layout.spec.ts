import { expect, test } from "@playwright/test";

import { apiCapabilitiesFixture } from "./apiCapabilities";
import { setAdminToken, setupApiMocks } from "./helpers";

test.use({ serviceWorkers: "block" });

const meshStatus = {
	generated_at: "2026-08-07T04:27:00Z",
	revision: 198_080,
	local: {
		node_id: "node-hk",
		node_name: "hk",
		cluster_id: "01KZ4S22RCWND8SE0000000000",
		role: "follower",
		leader_api_base_url: "https://101-xp.ivanli.cc",
		term: 63_193,
		mesh_proxy_status: "disabled",
		mesh_proxy_reason: null,
		canary: {
			enabled: true,
			bind: "127.0.0.1:39043",
			acme_directory_url: null,
			cert_not_after: "2026-11-01T00:00:00Z",
			last_renewed_at: "2026-08-01T00:00:00Z",
			last_error: null,
		},
	},
	peers: [
		{
			node_id: "node-101",
			node_name: "101",
			api_base_url: "https://101-xp.ivanli.cc",
			mesh_url: null,
			mesh_capability: "disabled",
			mesh_reason: "missing_endpoint",
			current_path: "public",
			quality: "unstable",
			stale: false,
			breaker: "disabled",
			last_sample_at: "2026-08-07T04:26:00Z",
			last_transition_at: "2026-08-04T05:44:00Z",
			availability_1h: 0.979,
			availability_24h: 0.986,
			mesh_availability_24h: null,
			latency_p50_ms: 1_184,
			latency_p95_ms: 3_067,
			buckets: [],
		},
		{
			node_id: "node-sg",
			node_name: "sg",
			api_base_url: "https://sg-ep.707979.xyz",
			mesh_url: "https://sg-ep.707979.xyz:443",
			mesh_capability: "enabled",
			mesh_reason: "mesh_available",
			current_path: "mesh",
			quality: "slow",
			stale: false,
			breaker: "closed",
			last_sample_at: "2026-08-07T04:26:00Z",
			last_transition_at: "2026-08-06T14:42:00Z",
			availability_1h: 1,
			availability_24h: 1,
			mesh_availability_24h: 1,
			latency_p50_ms: 155,
			latency_p95_ms: 303,
			mesh_transport: {
				protocol: "h2",
				health: "healthy",
				connection_generation: 4,
				current_connection_requests: 65,
				requests_5m: 65,
				connection_starts_5m: 1,
				requests_1h: 724,
				connection_starts_1h: 2,
				last_connection_started_at: "2026-08-07T04:02:00Z",
			},
			buckets: [],
		},
		{
			node_id: "node-jp",
			node_name: "jp",
			api_base_url: "https://jp-ep.707979.xyz",
			mesh_url: "https://jp-ep.707979.xyz:443",
			mesh_capability: "enabled",
			mesh_reason: "mesh_available",
			current_path: "mesh",
			quality: "good",
			stale: false,
			breaker: "closed",
			last_sample_at: "2026-08-07T04:26:00Z",
			last_transition_at: "2026-08-06T18:19:00Z",
			availability_1h: 1,
			availability_24h: 1,
			mesh_availability_24h: 1,
			latency_p50_ms: 207,
			latency_p95_ms: 276,
			buckets: [],
		},
	],
	events: [],
};

async function openSystemStatus(page: import("@playwright/test").Page) {
	await setAdminToken(page);
	await setupApiMocks(page, { mockStatusEvents: false });
	await page.route("**/api/capabilities", async (route) => {
		await route.fulfill({
			contentType: "application/json",
			body: JSON.stringify({
				...apiCapabilitiesFixture,
				capabilities: [
					...apiCapabilitiesFixture.capabilities,
					"admin.mesh-transport-reuse",
				],
			}),
		});
	});
	await page.route("**/api/admin/mesh/status", async (route) => {
		await route.fulfill({
			contentType: "application/json",
			body: JSON.stringify(meshStatus),
		});
	});
	await page.goto("/system-status");
	await expect(
		page.getByRole("heading", { name: "System status", exact: true }),
	).toBeVisible();
}

test("keeps peer row actions inside the real AppShell content column", async ({
	page,
}) => {
	await page.setViewportSize({ width: 1605, height: 806 });
	await page.emulateMedia({ colorScheme: "dark" });
	await openSystemStatus(page);
	await expect(page.locator("[data-peer-row]")).toHaveCount(
		meshStatus.peers.length,
	);
	await expect(
		page.locator('[data-mesh-transport="healthy"]:visible'),
	).toContainText("H2 · 65 req / 1 starts · gen 4");

	const bounds = await page.evaluate(() => {
		const panel = document.querySelector<HTMLElement>("main.xp-panel");
		if (!panel) throw new Error("AppShell main panel was not rendered");
		const panelRect = panel.getBoundingClientRect();
		const styles = window.getComputedStyle(panel);
		const contentRight =
			panelRect.right -
			Number.parseFloat(styles.paddingRight) -
			Number.parseFloat(styles.borderRightWidth);

		return [...document.querySelectorAll<HTMLElement>("[data-peer-row]")].map(
			(row) => {
				const details = row.querySelector<HTMLElement>(
					'[aria-label^="Open "][aria-label$=" details"]',
				);
				if (!details) throw new Error("Peer details action was not rendered");
				const rowRect = row.getBoundingClientRect();
				const detailsRect = details.getBoundingClientRect();
				return {
					peer: row.dataset.peerRow,
					contentRight,
					rowRight: rowRect.right,
					detailsRight: detailsRect.right,
					rowVisible: rowRect.width > 0 && rowRect.height > 0,
					detailsVisible: detailsRect.width > 0 && detailsRect.height > 0,
				};
			},
		);
	});
	for (const row of bounds) {
		expect(row.rowVisible, `${row.peer} row must be visible`).toBe(true);
		expect(
			row.detailsVisible,
			`${row.peer} details action must be visible`,
		).toBe(true);
		expect(
			row.rowRight,
			`${row.peer} row exceeds the panel content`,
		).toBeLessThanOrEqual(row.contentRight + 0.5);
		expect(
			row.detailsRight,
			`${row.peer} details action exceeds the panel content`,
		).toBeLessThanOrEqual(row.contentRight + 0.5);
	}
});

test("keeps the stacked mobile peer actions free of horizontal overflow", async ({
	page,
}) => {
	await page.setViewportSize({ width: 393, height: 852 });
	await openSystemStatus(page);

	await expect(
		page.getByRole("link", { name: "Details" }).first(),
	).toBeVisible();
	await expect(
		page.locator('[data-mesh-transport="healthy"]:visible'),
	).toContainText("H2 · 65 req / 1 starts · gen 4");
	const viewport = await page.evaluate(() => ({
		clientWidth: document.documentElement.clientWidth,
		scrollWidth: document.documentElement.scrollWidth,
	}));
	expect(viewport.scrollWidth).toBeLessThanOrEqual(viewport.clientWidth);
});
