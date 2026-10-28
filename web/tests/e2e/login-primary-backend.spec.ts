import { expect, test } from "@playwright/test";

import { fixtureCatalog } from "../../src/fixture-policy/catalog";
import { apiCapabilitiesFixture } from "./apiCapabilities";

const clusterId = fixtureCatalog.cluster.fixture84();
const recoveryOrigin = fixtureCatalog.url.secondaryApi();

function corsHeaders(origin: string) {
	return {
		"Access-Control-Allow-Origin": origin,
		"Access-Control-Allow-Methods": "GET, POST, PUT, PATCH, DELETE, OPTIONS",
		"Access-Control-Allow-Headers": "Authorization, Content-Type, Accept",
		Vary: "Origin, Access-Control-Request-Method, Access-Control-Request-Headers",
	};
}

function nodes(pageOrigin: string) {
	return [
		{
			node_id: fixtureCatalog.identifier.nodePrimary(),
			node_name: fixtureCatalog.identifier.nodeNamePrimary(),
			api_base_url: pageOrigin,
			access_host: fixtureCatalog.host.fixture88(),
			quota_limit_bytes: fixtureCatalog.quota.limitBytes(),
			quota_reset: fixtureCatalog.quota.resetUnlimited(),
		},
		{
			node_id: fixtureCatalog.identifier.nodeSecondary(),
			node_name: fixtureCatalog.identifier.nodeNameSecondary(),
			api_base_url: recoveryOrigin,
			access_host: fixtureCatalog.host.secondary(),
			quota_limit_bytes: fixtureCatalog.quota.limitBytes(),
			quota_reset: fixtureCatalog.quota.resetUnlimited(),
		},
	];
}

async function installLoginMocks(
	page: import("@playwright/test").Page,
	adminStatus: number,
) {
	const pageOrigin = new URL(
		process.env.E2E_BASE_URL ?? "http://127.0.0.1:60080",
	).origin;
	const nodeList = nodes(pageOrigin);
	await page.addInitScript(
		({ activeClusterId, activeRecoveryOrigin }) => {
			localStorage.removeItem("xp_admin_token");
			localStorage.setItem(
				"xp-primary-backend-profiles",
				JSON.stringify({
					activeClusterId,
					profiles: {
						[activeClusterId]: {
							clusterId: activeClusterId,
							primaryOrigin: location.origin,
							candidates: [
								{
									origin: location.origin,
									nodeId: "current-page",
									nodeName: "Current page",
									verifiedAt: Date.now(),
									lastError: null,
								},
								{
									origin: activeRecoveryOrigin,
									nodeId: "node-secondary",
									nodeName: "Secondary node",
									verifiedAt: Date.now(),
									lastError: null,
								},
							],
							updatedAt: Date.now(),
						},
					},
				}),
			);
		},
		{ activeClusterId: clusterId, activeRecoveryOrigin: recoveryOrigin },
	);

	await page.route(/^https?:\/\/[^/]+\/api\//, async (route) => {
		const request = route.request();
		const url = new URL(request.url());
		const origin = url.origin;
		const headers = corsHeaders(pageOrigin);
		if (request.method() === "OPTIONS") {
			await route.fulfill({ status: 204, headers });
			return;
		}
		if (url.pathname === "/api/health") {
			await route.fulfill({
				status: 200,
				headers,
				contentType: "application/json",
				body: JSON.stringify({ status: "ok" }),
			});
			return;
		}
		if (url.pathname === "/api/cluster/info") {
			await route.fulfill({
				status: 200,
				headers,
				contentType: "application/json",
				body: JSON.stringify({
					cluster_id: clusterId,
					node_id:
						origin === recoveryOrigin
							? fixtureCatalog.identifier.nodeSecondary()
							: fixtureCatalog.identifier.nodePrimary(),
					role: "leader",
					leader_api_base_url: recoveryOrigin,
					term: 1,
					xp_version: "v3.22.5",
				}),
			});
			return;
		}
		if (url.pathname === "/api/capabilities") {
			await route.fulfill({
				status: 200,
				headers,
				contentType: "application/json",
				body: JSON.stringify(apiCapabilitiesFixture),
			});
			return;
		}
		if (url.pathname === "/api/admin/alerts") {
			if (origin === recoveryOrigin && adminStatus !== 200) {
				await route.fulfill({
					status: adminStatus,
					headers,
					contentType: "application/json",
					body: JSON.stringify({
						error: {
							code: "unauthorized",
							message: "invalid admin token",
							details: {},
						},
					}),
				});
				return;
			}
			await route.fulfill({
				status: 200,
				headers,
				contentType: "application/json",
				body: JSON.stringify({ items: [] }),
			});
			return;
		}
		if (url.pathname === "/api/admin/nodes") {
			await route.fulfill({
				status: 200,
				headers,
				contentType: "application/json",
				body: JSON.stringify({ items: nodeList }),
			});
			return;
		}
		if (url.pathname === "/api/admin/status/events") {
			await route.fulfill({
				status: 200,
				headers: {
					...headers,
					"content-type": "text/event-stream",
					"cache-control": "no-cache",
				},
				body: "event: hello\\ndata: {}\\n\\n",
			});
			return;
		}
		await route.fulfill({
			status: 200,
			headers,
			contentType: "application/json",
			body: JSON.stringify({}),
		});
	});
}

test("uses selected recovery origin and persists after inventory hydration", async ({
	page,
}) => {
	const apiRequests: Array<{ url: string; authorization: string | null }> = [];
	page.on("request", (request) => {
		const url = new URL(request.url());
		if (url.pathname.startsWith("/api/")) {
			apiRequests.push({
				url: request.url(),
				authorization: request.headers().authorization ?? null,
			});
		}
	});
	await installLoginMocks(page, 200);
	await page.goto("/login", { waitUntil: "networkidle" });

	await page.getByRole("button", { name: "Select service node" }).click();
	await page
		.getByRole("menuitem", {
			name: /Secondary node/,
		})
		.click();
	await page.getByLabel("Token").fill("valid-admin-token");
	await page.getByRole("button", { name: "Save & Continue" }).click();

	await expect(page).toHaveURL(/\/$/);
	await expect
		.poll(() =>
			page.evaluate(
				({ key, expectedClusterId }) => {
					const raw = localStorage.getItem(key);
					if (!raw) return null;
					return JSON.parse(raw).profiles[expectedClusterId]?.primaryOrigin;
				},
				{ key: "xp-primary-backend-profiles", expectedClusterId: clusterId },
			),
		)
		.toBe(recoveryOrigin);

	const authRequest = apiRequests.find((request) =>
		request.url.startsWith(`${recoveryOrigin}/api/admin/alerts`),
	);
	expect(authRequest?.authorization).toBe("Bearer valid-admin-token");
	const alertRequests = apiRequests.filter(
		(request) => new URL(request.url).pathname === "/api/admin/alerts",
	);
	expect(alertRequests.length).toBeGreaterThan(0);
	expect(
		alertRequests.every(
			(request) => request.url === `${recoveryOrigin}/api/admin/alerts`,
		),
	).toBe(true);
});

test("keeps the login page and selected origin after a recovery 401", async ({
	page,
}) => {
	const apiRequests: string[] = [];
	page.on("request", (request) => {
		const url = new URL(request.url());
		if (url.pathname.startsWith("/api/")) apiRequests.push(request.url());
	});
	await installLoginMocks(page, 401);
	await page.goto("/login", { waitUntil: "networkidle" });

	await page.getByRole("button", { name: "Select service node" }).click();
	await page
		.getByRole("menuitem", {
			name: /Secondary node/,
		})
		.click();
	await page.getByLabel("Token").fill("invalid-admin-token");
	await page.getByRole("button", { name: "Save & Continue" }).click();

	await expect(
		page.getByText(/401 unauthorized: invalid admin token/),
	).toBeVisible();
	await expect(page).toHaveURL(/\/login$/);
	expect(
		await page.evaluate(() => localStorage.getItem("xp_admin_token")),
	).toBeNull();
	expect(
		apiRequests.some((url) =>
			url.startsWith(`${recoveryOrigin}/api/admin/alerts`),
		),
	).toBe(true);
	const alertRequests = apiRequests.filter(
		(url) => new URL(url).pathname === "/api/admin/alerts",
	);
	expect(alertRequests.length).toBeGreaterThan(0);
	expect(
		alertRequests.every((url) => url === `${recoveryOrigin}/api/admin/alerts`),
	).toBe(true);
});

test("keeps login selector focus and controls inside the mobile viewport", async ({
	page,
}) => {
	await page.setViewportSize({ width: 393, height: 852 });
	await installLoginMocks(page, 200);
	await page.goto("/login", { waitUntil: "networkidle" });

	const selector = page.getByRole("button", { name: "Select service node" });
	await selector.click();
	await expect(page.getByRole("menu")).toBeVisible();
	await page.keyboard.press("Escape");
	await expect(selector).toBeFocused();

	const layout = await page.evaluate(() => {
		const controls = [
			document.querySelector('button[aria-label="Select service node"]'),
			document.querySelector('input[name="token"]'),
		];
		return {
			clientWidth: document.documentElement.clientWidth,
			scrollWidth: document.documentElement.scrollWidth,
			rects: controls.map((element) => {
				if (!element) return null;
				const box = element.getBoundingClientRect();
				return { left: box.left, right: box.right };
			}),
		};
	});

	expect(layout.scrollWidth).toBeLessThanOrEqual(layout.clientWidth);
	for (const rect of layout.rects) {
		expect(rect).not.toBeNull();
		expect(rect?.left).toBeGreaterThanOrEqual(0);
		expect(rect?.right).toBeLessThanOrEqual(layout.clientWidth);
	}
});
