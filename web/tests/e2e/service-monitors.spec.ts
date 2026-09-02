import { type Page, type Route, expect, test } from "@playwright/test";

import { fixtureCatalog } from "../../src/fixture-policy/catalog";
import { apiCapabilitiesFixture } from "./apiCapabilities";
import { setAdminToken, setupApiMocks } from "./helpers";

const monitorId = "01JMONITOR00000000000000001";
const now = 1_785_278_400;

type Monitor = {
	monitor_id: string;
	name: string;
	target:
		| {
				kind: "https";
				url: string;
				method: "get" | "head";
				accepted_statuses: Array<{ start: number; end: number }>;
		  }
		| { kind: "tcping"; host: string; port: number };
	interval_seconds: number;
	observer_policy: { mode: "exclude" | "include"; node_ids: string[] };
	lifecycle: "active" | "paused" | "deleted";
	revision: number;
	revision_effective_at_unix_seconds: number;
};

function json(route: Route, body: unknown, status = 200) {
	return route.fulfill({
		status,
		contentType: "application/json",
		body: JSON.stringify(body),
	});
}

function monitorStatus(monitor: Monitor) {
	return {
		monitor_id: monitor.monitor_id,
		status: "up",
		stale: false,
		freshness_seconds: 18,
		capture: {
			suspended: false,
			pending_observations: 0,
			pending_bytes: 0,
		},
		quality: "complete",
		observers: [
			{
				node_id: fixtureCatalog.identifier.nodePrimary(),
				state: "up",
				latest: {
					monitor_id: monitor.monitor_id,
					revision: monitor.revision,
					observer_node_id: fixtureCatalog.identifier.nodePrimary(),
					slot_unix_seconds: now - 60,
					observed_at_unix_seconds: now - 18,
					outcome: "success",
					error: null,
					latency_ms: fixtureCatalog.number.value42(),
					status_code: 200,
					packet_loss_percent: 0,
					ad_hoc: false,
				},
				icmp_supported: true,
			},
		],
	};
}

function monitorHistory(monitor: Monitor) {
	return {
		monitor_id: monitor.monitor_id,
		resolution: "1m",
		points: [
			{
				start_unix_seconds: now - 60,
				end_unix_seconds: now - 1,
				rollup: {
					expected: 1,
					executed: 1,
					successes: 1,
					failures: 0,
					unsupported: 0,
					suspended: 0,
					latency_count: 1,
					latency_sum_ms: fixtureCatalog.number.value42(),
					latency_min_ms: fixtureCatalog.number.value42(),
					latency_max_ms: fixtureCatalog.number.value42(),
					latency_histogram: {
						underflow: 0,
						buckets: Array.from({ length: 32 }, (_, index) =>
							index === 5 ? 1 : 0,
						),
						overflow: 0,
					},
					errors: {},
				},
				availability_percent: 100,
				coverage_percent: 100,
			},
		],
		truncated: false,
		quality: "complete",
		coverage_percent: 100,
		watermark_unix_seconds: now - 18,
		gaps: [],
		skew_seconds: 0,
		freshness_seconds: 18,
	};
}

function recentSummary() {
	return {
		availability_percent: 100,
		coverage_percent: 100,
		expected: 360,
		executed: 360,
		latest_latency_ms: fixtureCatalog.number.value42(),
		latest_observed_at_unix_seconds: now - 18,
		slots: Array.from({ length: 72 }, () => "up"),
	};
}

async function setupServiceMonitorMocks(page: Page) {
	await setAdminToken(page);
	await setupApiMocks(page, { mockStatusEvents: false });
	await page.route("**/api/capabilities", (route) =>
		json(route, {
			...apiCapabilitiesFixture,
			capabilities: [
				...apiCapabilitiesFixture.capabilities,
				"admin.service-monitors",
				"admin.service-monitor-observer-policy-v1",
				"admin.service-monitor-draft-tests-v1",
			],
		}),
	);
	let draftTest: Record<string, unknown> | null = null;
	await page.route("**/api/admin/monitor-draft-tests**", async (route) => {
		const request = route.request();
		if (request.method() === "POST") {
			draftTest = {
				run_id: "01JDRAFT0000000000000000001",
				target: JSON.parse(request.postData() ?? "{}").target,
				observer_policy: { mode: "exclude", node_ids: [] },
				observer_node_ids: [fixtureCatalog.identifier.nodePrimary()],
				coordinator_node_id: fixtureCatalog.identifier.nodePrimary(),
				state: "succeeded",
				created_at_unix_seconds: now,
				expires_at_unix_seconds: now + 900,
				observers: [
					{
						node_id: fixtureCatalog.identifier.nodePrimary(),
						state: "succeeded",
						latency_ms: 42,
						status_code: 200,
					},
				],
			};
			return json(route, draftTest, 202);
		}
		return draftTest
			? json(route, draftTest)
			: json(
					route,
					{ error: { code: "not_found", message: "not found", details: {} } },
					404,
				);
	});

	let monitor: Monitor = {
		monitor_id: monitorId,
		name: "Public API health",
		target: {
			kind: "https",
			url: "https://status.example.net/health",
			method: "get",
			accepted_statuses: [{ start: 200, end: 399 }],
		},
		interval_seconds: 60,
		observer_policy: { mode: "exclude", node_ids: [] },
		lifecycle: "active",
		revision: 3,
		revision_effective_at_unix_seconds: now - 60,
	};

	await page.route("**/api/admin/monitors**", async (route) => {
		const request = route.request();
		const url = new URL(request.url());
		const { pathname } = url;
		if (
			pathname === "/api/admin/monitors/test" &&
			request.method() === "POST"
		) {
			const payload = JSON.parse(request.postData() ?? "{}") as {
				target: Monitor["target"];
			};
			return json(route, {
				target: payload.target,
				observations: [
					{
						monitor_id: "01JTEST0000000000000000001",
						revision: 1,
						observer_node_id: fixtureCatalog.identifier.nodePrimary(),
						slot_unix_seconds: now,
						observed_at_unix_seconds: now,
						outcome: "success",
						error: null,
						latency_ms: fixtureCatalog.number.value42(),
						status_code: 200,
						packet_loss_percent: 0,
						ad_hoc: true,
					},
				],
			});
		}
		if (pathname === "/api/admin/monitors" && request.method() === "GET") {
			return json(route, {
				items: [
					{
						...monitor,
						status: "up",
						stale: false,
						quality: "complete",
						recent_6h: recentSummary(),
					},
				],
			});
		}
		if (pathname === `/api/admin/monitors/${monitorId}/status`) {
			return json(route, monitorStatus(monitor));
		}
		if (pathname === `/api/admin/monitors/${monitorId}/history`) {
			return json(route, monitorHistory(monitor));
		}
		if (pathname === `/api/admin/monitors/${monitorId}/run`) {
			return json(
				route,
				{
					run_id: fixtureCatalog.identifier.probeRunPrimary(),
					state: "queued",
				},
				202,
			);
		}
		if (pathname !== `/api/admin/monitors/${monitorId}`) {
			return json(
				route,
				{ error: { code: "not_found", message: "not found", details: {} } },
				404,
			);
		}
		if (request.method() === "GET") return json(route, monitor);
		if (request.method() === "DELETE") {
			monitor = {
				...monitor,
				lifecycle: "deleted",
				revision: monitor.revision + 1,
			};
			return route.fulfill({ status: 204 });
		}
		if (request.method() === "PATCH") {
			const payload = JSON.parse(
				request.postData() ?? "{}",
			) as Partial<Monitor>;
			monitor = {
				...monitor,
				...payload,
				revision: monitor.revision + 1,
				revision_effective_at_unix_seconds: now + 60,
			};
			return json(route, monitor);
		}
		return json(
			route,
			{
				error: { code: "invalid_request", message: "unsupported", details: {} },
			},
			400,
		);
	});
}

test("edits, pauses, and runs a service monitor", async ({ page }) => {
	await setupServiceMonitorMocks(page);
	await page.goto("/monitors");
	await expect(
		page.getByRole("heading", { name: "Service monitoring" }),
	).toBeVisible();

	await page.getByRole("link", { name: "Public API health" }).click();
	await expect(
		page.getByRole("heading", { name: "Public API health" }),
	).toBeVisible();
	await expect(page.getByLabel("Service monitor history chart")).toBeVisible();
	await expect(
		page.getByRole("heading", { name: "Observer results" }),
	).toBeVisible();

	await page.getByRole("button", { name: "Pause" }).click();
	await expect(page.getByRole("button", { name: "Resume" })).toBeVisible();
	await page.getByRole("link", { name: "Edit" }).click();
	await page.getByLabel("Name").fill("Public API health updated");
	await page.getByRole("button", { name: "Save changes" }).click();
	await expect(
		page.getByRole("heading", { name: "Public API health updated" }),
	).toBeVisible();

	await page.getByRole("button", { name: "Run now" }).click();
	await expect(page.getByText("Check started.")).toBeVisible();
});

test("keeps the TCPING editor usable on mobile", async ({ page }) => {
	await page.setViewportSize({ width: 393, height: 852 });
	await setupServiceMonitorMocks(page);
	await page.goto("/monitors/new");
	await expect(
		page.getByRole("heading", { name: "New service monitor" }),
	).toBeVisible();
	await page.getByRole("combobox", { name: "Method", exact: true }).click();
	await page.getByRole("option", { name: "TCPING" }).click();
	await expect(page.getByLabel("TCP port")).toBeVisible();

	const viewport = await page.evaluate(() => ({
		clientWidth: document.documentElement.clientWidth,
		scrollWidth: document.documentElement.scrollWidth,
	}));
	expect(viewport.scrollWidth).toBeLessThanOrEqual(viewport.clientWidth);
});

test("inverts observer choices when changing policy mode", async ({ page }) => {
	await setupServiceMonitorMocks(page);
	await page.goto("/monitors/new");

	const observer = page.getByRole("checkbox");
	await expect(observer).not.toBeChecked();
	await page.getByRole("tab", { name: "Include only" }).click();
	await expect(observer).toBeChecked();
	await page.getByRole("tab", { name: "Exclude nodes" }).click();
	await expect(observer).not.toBeChecked();
});

test("runs the backend cluster test from the B editor workspace", async ({
	page,
}) => {
	await page.setViewportSize({ width: 1536, height: 1000 });
	await setupServiceMonitorMocks(page);
	await page.goto("/monitors/new");

	const configuration = page.getByRole("heading", {
		name: "Monitor configuration",
	});
	const results = page.getByRole("heading", { name: "Cluster test results" });
	await expect(configuration).toBeVisible();
	await expect(results).toBeVisible();
	const columns = await page.evaluate(() => {
		const configurationHeading = document.querySelector(
			"#monitor-configuration-heading",
		);
		const resultsHeading = document.querySelector(
			"#monitor-cluster-test-heading",
		);
		if (!configurationHeading || !resultsHeading) {
			throw new Error("editor columns are not mounted");
		}
		return {
			configurationLeft: configurationHeading.getBoundingClientRect().left,
			resultsLeft: resultsHeading.getBoundingClientRect().left,
		};
	});
	expect(columns.resultsLeft).toBeGreaterThan(columns.configurationLeft);
	const policyTabs = page.getByRole("tablist");
	await expect(policyTabs).toBeVisible();
	const policyLayout = await page.evaluate(() => {
		const tablist = document.querySelector('[role="tablist"]');
		const configuration = document
			.querySelector("#monitor-configuration-heading")
			?.closest("section");
		if (!tablist || !configuration) {
			throw new Error("observer policy tabs are not mounted");
		}
		return {
			tablistWidth: tablist.getBoundingClientRect().width,
			configurationWidth: configuration.getBoundingClientRect().width,
		};
	});
	expect(policyLayout.tablistWidth).toBeLessThan(
		policyLayout.configurationWidth,
	);
	const compactPolicyLayout = await page.evaluate(() => {
		const heading = document.querySelector("#monitor-observer-policy-heading");
		const tablist = document.querySelector('[role="tablist"]');
		const nodes = heading?.closest("section")?.querySelector("fieldset");
		if (!heading || !tablist || !nodes) {
			throw new Error("compact observer policy controls are not mounted");
		}
		return {
			headingTop: heading.getBoundingClientRect().top,
			tablistTop: tablist.getBoundingClientRect().top,
			tablistRight: tablist.getBoundingClientRect().right,
			nodesTop: nodes.getBoundingClientRect().top,
			sectionRight: heading.closest("section")?.getBoundingClientRect().right,
		};
	});
	expect(
		Math.abs(compactPolicyLayout.headingTop - compactPolicyLayout.tablistTop),
	).toBeLessThanOrEqual(8);
	expect(
		compactPolicyLayout.nodesTop - compactPolicyLayout.headingTop,
	).toBeLessThanOrEqual(80);
	expect(compactPolicyLayout.sectionRight).toBeDefined();
	expect(
		Math.abs(
			compactPolicyLayout.tablistRight -
				(compactPolicyLayout.sectionRight ?? 0),
		),
	).toBeLessThanOrEqual(2);
	await page.getByRole("button", { name: "Run cluster test" }).click();

	await expect(
		page.getByText("1 / 1 observers reached the target"),
	).toBeVisible();
	await expect(
		page.getByText(fixtureCatalog.identifier.nodePrimary(), { exact: true }),
	).toBeVisible();
	await expect(page.getByText("HTTP 200", { exact: true })).toBeVisible();
});
