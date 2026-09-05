import { type Page, expect, test } from "@playwright/test";

import { fixtureCatalog } from "../../src/fixture-policy/catalog";
import { apiCapabilitiesFixture } from "./apiCapabilities";
import { type AdminNode, setAdminToken, setupApiMocks } from "./helpers";

function corsHeaders(origin: string) {
	return {
		"Access-Control-Allow-Origin": origin,
		"Access-Control-Allow-Methods": "GET, POST, PUT, PATCH, DELETE, OPTIONS",
		"Access-Control-Allow-Headers": "Authorization, Content-Type, Accept",
		Vary: "Origin, Access-Control-Request-Method, Access-Control-Request-Headers",
	};
}

function documentCsp(connectOrigin: string) {
	return [
		"default-src 'self'",
		"base-uri 'self'",
		"object-src 'none'",
		"frame-ancestors 'none'",
		`connect-src 'self'${connectOrigin ? ` ${connectOrigin}` : ""}`,
		"img-src 'self' data: blob:",
		"script-src 'self' 'unsafe-inline'",
		"style-src 'self' 'unsafe-inline'",
		"font-src 'self'",
	].join("; ");
}

async function injectDocumentCsp(page: Page, connectOrigin: string) {
	const appOrigin = new URL(
		process.env.E2E_BASE_URL ?? "http://127.0.0.1:60080",
	).origin;
	await page.route(`${appOrigin}/`, async (route) => {
		const response = await route.fetch();
		await route.fulfill({
			response,
			headers: {
				...response.headers(),
				"content-security-policy": documentCsp(connectOrigin),
			},
		});
	});
}

test("switches API and SSE traffic to a verified recovery origin", async ({
	page,
}) => {
	const recoveryOrigin = fixtureCatalog.url.secondaryApi();
	const nodes: AdminNode[] = [
		{
			node_id: fixtureCatalog.identifier.nodePrimary(),
			node_name: fixtureCatalog.identifier.nodeNamePrimary(),
			api_base_url: fixtureCatalog.service.fixture87(),
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
	await setAdminToken(page);
	await setupApiMocks(page, { nodes, mockStatusEvents: false });
	await injectDocumentCsp(page, recoveryOrigin);

	await page.route(`${recoveryOrigin}/**`, async (route) => {
		const request = route.request();
		const url = new URL(request.url());
		const origin = new URL(page.url()).origin;
		const headers = corsHeaders(origin);
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
					node_id: fixtureCatalog.identifier.nodeSecondary(),
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
		if (url.pathname === "/api/admin/nodes") {
			await route.fulfill({
				status: 200,
				headers,
				contentType: "application/json",
				body: JSON.stringify({ items: nodes }),
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

	const apiRequests: string[] = [];
	page.on("request", (request) => {
		if (new URL(request.url()).pathname.startsWith("/api/")) {
			apiRequests.push(request.url());
		}
	});

	await page.goto("/");
	await expect(
		page.getByRole("heading", { name: "Dashboard", exact: true }),
	).toBeVisible();
	await page.getByRole("button", { name: "Open primary backend" }).click();
	await page
		.getByRole("menuitem")
		.filter({ hasText: fixtureCatalog.identifier.nodeSecondary() })
		.click();

	const switcher = page.getByRole("button", { name: "Open primary backend" });
	await expect(switcher).toHaveAttribute(
		"title",
		`Primary backend: ${new URL(recoveryOrigin).host}`,
	);
	await expect
		.poll(
			() =>
				apiRequests.filter((url) => url.startsWith(`${recoveryOrigin}/api/`))
					.length,
		)
		.toBeGreaterThan(0);
	await expect
		.poll(() =>
			apiRequests.some(
				(url) => url === `${recoveryOrigin}/api/admin/status/events`,
			),
		)
		.toBe(true);
});

test("blocks an unlisted recovery origin at the document CSP", async ({
	page,
}) => {
	const recoveryOrigin = fixtureCatalog.url.secondaryApi();
	await page.route(`${recoveryOrigin}/api/health`, async (route) => {
		await route.fulfill({
			status: 200,
			headers: corsHeaders(new URL(page.url()).origin),
			contentType: "application/json",
			body: JSON.stringify({ status: "ok" }),
		});
	});
	await injectDocumentCsp(page, "");
	await page.goto("/");

	const result = await page.evaluate(async (origin) => {
		try {
			await fetch(`${origin}/api/health`);
			return "ok";
		} catch (error) {
			return String(error);
		}
	}, recoveryOrigin);
	expect(result).toContain("TypeError");
});
