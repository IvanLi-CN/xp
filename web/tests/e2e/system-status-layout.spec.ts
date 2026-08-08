import { expect, test } from "@playwright/test";

import { apiCapabilitiesFixture } from "./apiCapabilities";
import { fixtureCatalog } from "@/fixture-policy/catalog";
import { setAdminToken, setupApiMocks } from "./helpers";

test.use({ serviceWorkers: "block" });

const meshStatus = {
	generated_at: fixtureCatalog.timestamp.recent(),
	revision: 198_080,
	local: {
		node_id: fixtureCatalog.identifier.nodePrimary(),
		node_name: fixtureCatalog.identifier.nodePrimary(),
		cluster_id: fixtureCatalog.identifier.clusterPrimary(),
		role: "follower",
		leader_api_base_url: fixtureCatalog.url.primaryApi(),
		term: 63_193,
		mesh_proxy_status: "disabled",
		mesh_proxy_reason: null,
		canary: {
			enabled: true,
			bind: fixtureCatalog.address.loopback39043(),
			acme_directory_url: null,
			cert_not_after: fixtureCatalog.timestamp.recent(),
			last_renewed_at: fixtureCatalog.timestamp.baseline(),
			last_error: null,
		},
	},
	peers: [
		{
			node_id: fixtureCatalog.identifier.nodeSecondary(),
			node_name: fixtureCatalog.identifier.nodeSecondary(),
			api_base_url: fixtureCatalog.url.secondaryApi(),
			mesh_url: fixtureCatalog.url.none(),
			mesh_capability: "disabled",
			mesh_reason: "missing_endpoint",
			current_path: "public",
			quality: "unstable",
			stale: false,
			breaker: "disabled",
			last_sample_at: fixtureCatalog.timestamp.recent(),
			last_transition_at: fixtureCatalog.timestamp.baseline(),
			availability_1h: fixtureCatalog.metric.availabilityLow(),
			availability_24h: fixtureCatalog.metric.availabilityHigh(),
			mesh_availability_24h: fixtureCatalog.metric.none(),
			latency_p50_ms: fixtureCatalog.metric.latencyHigh(),
			latency_p95_ms: fixtureCatalog.metric.latencyHigh(),
			buckets: [],
		},
		{
			node_id: fixtureCatalog.identifier.nodeTertiary(),
			node_name: fixtureCatalog.identifier.nodeTertiary(),
			api_base_url: fixtureCatalog.url.tertiaryApi(),
			mesh_url: fixtureCatalog.url.tertiaryApi(),
			mesh_capability: "enabled",
			mesh_reason: "mesh_available",
			current_path: "mesh",
			quality: "slow",
			stale: false,
			breaker: "closed",
			last_sample_at: fixtureCatalog.timestamp.recent(),
			last_transition_at: fixtureCatalog.timestamp.baseline(),
			availability_1h: fixtureCatalog.metric.availabilityFull(),
			availability_24h: fixtureCatalog.metric.availabilityFull(),
			mesh_availability_24h: fixtureCatalog.metric.availabilityFull(),
			latency_p50_ms: fixtureCatalog.metric.latencyLow(),
			latency_p95_ms: fixtureCatalog.metric.latencyHigh(),
			mesh_transport: {
				protocol: "h2",
				health: "healthy",
				connection_generation: 4,
				current_connection_requests: 65,
				requests_5m: 65,
				connection_starts_5m: 1,
				requests_1h: 724,
				connection_starts_1h: 2,
				last_connection_started_at: fixtureCatalog.timestamp.recent(),
			},
			buckets: [],
		},
		{
			node_id: fixtureCatalog.identifier.nodePrimary(),
			node_name: fixtureCatalog.identifier.nodePrimary(),
			api_base_url: fixtureCatalog.url.primaryApi(),
			mesh_url: fixtureCatalog.url.primaryApi(),
			mesh_capability: "enabled",
			mesh_reason: "mesh_available",
			current_path: "mesh",
			quality: "good",
			stale: false,
			breaker: "closed",
			last_sample_at: fixtureCatalog.timestamp.recent(),
			last_transition_at: fixtureCatalog.timestamp.baseline(),
			availability_1h: fixtureCatalog.metric.availabilityFull(),
			availability_24h: fixtureCatalog.metric.availabilityFull(),
			mesh_availability_24h: fixtureCatalog.metric.availabilityFull(),
			latency_p50_ms: fixtureCatalog.metric.latencyHigh(),
			latency_p95_ms: fixtureCatalog.metric.latencyHigh(),
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
