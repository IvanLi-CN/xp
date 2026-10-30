import { expect, test } from "@playwright/test";

import { fixtureCatalog } from "../../src/fixture-policy/catalog";
import { apiCapabilitiesFixture } from "./apiCapabilities";
import { statusEventsFixture } from "./statusEvents";

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

function nodes() {
	return [
		{
			node_id: fixtureCatalog.identifier.nodePrimary(),
			node_name: fixtureCatalog.identifier.nodeNamePrimary(),
			api_base_url: fixtureCatalog.url.primaryApi(),
			access_host: fixtureCatalog.host.fixture88(),
			quota_limit_bytes: fixtureCatalog.quota.limitBytes(),
			quota_reset: fixtureCatalog.quota.resetUnlimited(),
		},
		{
			node_id: fixtureCatalog.identifier.nodeSecondary(),
			node_name: fixtureCatalog.identifier.nodeNameSecondary(),
			api_base_url: fixtureCatalog.url.secondaryApi(),
			access_host: fixtureCatalog.host.secondary(),
			quota_limit_bytes: fixtureCatalog.quota.limitBytes(),
			quota_reset: fixtureCatalog.quota.resetUnlimited(),
		},
	];
}

async function installLoginMocks(
	page: import("@playwright/test").Page,
	adminStatus: number,
	options: {
		restoreRemote?: boolean;
		remoteUnreachable?: boolean;
		authenticated?: boolean;
	} = {},
) {
	const pageOrigin = new URL(
		process.env.E2E_BASE_URL ?? "http://127.0.0.1:60080",
	).origin;
	const nodeList = nodes();
	const initialProfile = {
		clusterId,
		primaryOrigin: pageOrigin,
		candidates: [
			{
				origin: pageOrigin,
				nodeId: fixtureCatalog.identifier.nodePrimary(),
				nodeName: fixtureCatalog.identifier.nodeNamePrimary(),
				verifiedAt: Date.now(),
				lastError: null,
			},
			{
				origin: recoveryOrigin,
				nodeId: fixtureCatalog.identifier.nodeSecondary(),
				nodeName: fixtureCatalog.label.tokyo(),
				verifiedAt: Date.now(),
				lastError: null,
			},
		],
		updatedAt: fixtureCatalog.number.value100(),
	};
	await page.addInitScript(
		({ activeClusterId, profile, restoreRemote, authenticated }) => {
			if (authenticated)
				localStorage.setItem("xp_admin_token", "valid-admin-token");
			else localStorage.removeItem("xp_admin_token");
			if (!restoreRemote) profile.primaryOrigin = location.origin;
			profile.candidates[0].origin = location.origin;
			localStorage.setItem(
				"xp-primary-backend-profiles",
				JSON.stringify({
					activeClusterId,
					profiles: Object.fromEntries([[activeClusterId, profile]]),
				}),
			);
		},
		{
			activeClusterId: clusterId,
			profile: {
				...initialProfile,
				primaryOrigin: options.restoreRemote ? recoveryOrigin : pageOrigin,
			},
			restoreRemote: options.restoreRemote ?? false,
			authenticated: options.authenticated ?? false,
		},
	);

	await page.route(/^https?:\/\/[^/]+\/api\//, async (route) => {
		const request = route.request();
		const url = new URL(request.url());
		const origin = url.origin;
		const headers = corsHeaders(pageOrigin);
		if (origin === recoveryOrigin && options.remoteUnreachable) {
			await route.abort("failed");
			return;
		}
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
					cluster_id: fixtureCatalog.cluster.fixture84(),
					node_id:
						origin === recoveryOrigin
							? fixtureCatalog.identifier.nodeSecondary()
							: fixtureCatalog.identifier.nodePrimary(),
					role: "leader",
					leader_api_base_url: fixtureCatalog.url.secondaryApi(),
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
				body: statusEventsFixture(),
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

test("clears a restored session before sending credentials to a different cluster", async ({
	page,
}) => {
	await installLoginMocks(page, 200, {
		restoreRemote: true,
		authenticated: true,
	});
	const credentialedRequests: string[] = [];
	page.on("request", (request) => {
		if (
			new URL(request.url()).origin === recoveryOrigin &&
			request.headers().authorization
		) {
			credentialedRequests.push(request.url());
		}
	});
	await page.route("**/api/cluster/info", async (route) => {
		if (new URL(route.request().url()).origin !== recoveryOrigin)
			return route.fallback();
		await route.fulfill({
			status: 200,
			headers: corsHeaders(new URL(page.url()).origin),
			contentType: "application/json",
			body: JSON.stringify({
				cluster_id: fixtureCatalog.cluster.fixture95(),
				node_id: fixtureCatalog.identifier.nodeSecondary(),
				role: "leader",
				leader_api_base_url: fixtureCatalog.url.secondaryApi(),
				term: 1,
				xp_version: "v3.22.5",
			}),
		});
	});
	await page.goto("/");
	await expect(page).toHaveURL(/\/login/);
	await expect
		.poll(() => page.evaluate(() => localStorage.getItem("xp_admin_token")))
		.toBeNull();
	expect(credentialedRequests).toEqual([]);
});

test("recovers to the page origin when the restored primary is unreachable", async ({
	page,
}) => {
	await installLoginMocks(page, 200, {
		restoreRemote: true,
		remoteUnreachable: true,
	});
	await page.goto("/login");
	await page.getByRole("button", { name: "Select service node" }).click();
	await page
		.getByRole("menuitem", {
			name: new RegExp(fixtureCatalog.identifier.nodeNamePrimary()),
		})
		.click();
	await page.getByLabel("Token").fill("valid-admin-token");
	const login = page.waitForRequest(
		(request) => new URL(request.url()).pathname === "/api/admin/alerts",
	);
	await page.getByRole("button", { name: "Save & Continue" }).click();
	expect(new URL((await login).url()).origin).toBe(new URL(page.url()).origin);
	await expect(page).toHaveURL(/\/$/);
});

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
	await expect
		.poll(
			() =>
				apiRequests.filter((request) =>
					["/api/health", "/api/cluster/info", "/api/capabilities"].includes(
						new URL(request.url).pathname,
					),
				).length,
		)
		.toBeGreaterThan(0);
	const preAuthProbeRequests = apiRequests.filter((request) =>
		["/api/health", "/api/cluster/info", "/api/capabilities"].includes(
			new URL(request.url).pathname,
		),
	);
	expect(preAuthProbeRequests.length).toBeGreaterThan(0);
	expect(
		preAuthProbeRequests.every((request) => request.authorization === null),
	).toBe(true);
	apiRequests.length = 0;

	const desktopSelector = page.getByRole("button", {
		name: "Select service node",
	});
	const desktopBounds = await desktopSelector.boundingBox();
	expect(desktopBounds).not.toBeNull();
	if (!desktopBounds) throw new Error("Service node selector is not visible");
	expect(desktopBounds.y).toBeLessThan(150);
	expect(desktopBounds.x + desktopBounds.width).toBeGreaterThan(1240);
	await desktopSelector.click();
	const menuBounds = await page.getByRole("menu").boundingBox();
	expect(menuBounds).not.toBeNull();
	if (!menuBounds) throw new Error("Service node menu is not visible");
	expect(Math.abs(menuBounds.width - desktopBounds.width)).toBeLessThan(1);
	expect(
		await page
			.getByRole("menu")
			.evaluate((menu) =>
				[...menu.querySelectorAll("[role=menuitem] .truncate")].every(
					(item) => item.scrollWidth <= item.clientWidth,
				),
			),
	).toBe(true);
	await page
		.getByRole("menuitem", {
			name: /Tokyo/,
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
	await expect
		.poll(
			() =>
				apiRequests.filter(
					(request) =>
						new URL(request.url).pathname === "/api/admin/status/events",
				).length,
		)
		.toBeGreaterThan(0);
	const sseRequests = apiRequests.filter(
		(request) => new URL(request.url).pathname === "/api/admin/status/events",
	);
	expect(
		sseRequests.every(
			(request) => new URL(request.url).origin === recoveryOrigin,
		),
	).toBe(true);
	await expect(page.getByText("Term: 73")).toBeVisible();
});

test("does not send a token when the selected node changes clusters before login", async ({
	page,
}) => {
	await installLoginMocks(page, 200);
	let changedCluster = false;
	const adminRequests: string[] = [];
	page.on("request", (request) => {
		if (new URL(request.url()).pathname.startsWith("/api/admin/")) {
			adminRequests.push(request.url());
		}
	});
	await page.route("**/api/cluster/info", async (route) => {
		if (
			changedCluster &&
			new URL(route.request().url()).origin === recoveryOrigin
		) {
			await route.fulfill({
				status: 200,
				headers: corsHeaders(new URL(page.url()).origin),
				contentType: "application/json",
				body: JSON.stringify({
					cluster_id: fixtureCatalog.cluster.fixture95(),
				}),
			});
			return;
		}
		await route.fallback();
	});
	await page.goto("/login", { waitUntil: "networkidle" });
	await page.getByRole("button", { name: "Select service node" }).click();
	await page.getByRole("menuitem", { name: /Tokyo/ }).click();
	await page.getByLabel("Token").fill("valid-admin-token");
	await expect(
		page.getByRole("button", { name: "Save & Continue" }),
	).toBeEnabled();
	changedCluster = true;
	await page.getByRole("button", { name: "Save & Continue" }).click();
	await expect(
		page.getByText("Backend belongs to a different cluster."),
	).toBeVisible();
	expect(adminRequests).toEqual([]);
	expect(
		await page.evaluate(() => localStorage.getItem("xp_admin_token")),
	).toBeNull();
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
			name: /Tokyo/,
		})
		.click();
	await page.getByLabel("Token").fill("invalid-admin-token");
	await page.getByRole("button", { name: "Save & Continue" }).click();

	await expect(
		page.getByText(/401 unauthorized: invalid admin token/),
	).toBeVisible();
	await expect(page).toHaveURL(/\/login$/);
	const selector = page.getByRole("button", { name: "Select service node" });
	await expect(selector).toContainText("Tokyo");
	await selector.press("Enter");
	await expect(page.getByRole("menu")).toBeVisible();
	await page.keyboard.press("Escape");
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
	await selector.press("Enter");
	await expect(page.getByRole("menu")).toBeVisible();
	await page.keyboard.press("ArrowDown");
	await expect(page.locator('[role="menuitem"]:focus')).toBeVisible();
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
