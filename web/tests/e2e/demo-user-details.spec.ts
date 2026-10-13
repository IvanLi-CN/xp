import { expect, test } from "@playwright/test";
import { fixtureCatalog } from "../../src/fixture-policy/catalog";

const userId = fixtureCatalog.identifier.userTertiary();

test("demo opens the formal user route without a login gate", async ({
	page,
}) => {
	await page.goto(`/demo/users/${userId}?demo=normal&d_profile=valid`);

	await expect(page.getByRole("heading", { name: "佐藤 未来" })).toBeVisible();
	await expect(page.getByRole("button", { name: "Reset token" })).toBeVisible();
	await expect(
		page.getByRole("button", { name: "Reset credentials" }),
	).toBeVisible();
	await expect(page.getByTestId("subscription-format")).toBeVisible();
});

test("demo remains isolated from an existing live service worker", async ({
	page,
}) => {
	await page.goto(`/demo/users/${userId}?demo=normal`);
	await page.evaluate(async () => {
		await navigator.serviceWorker.register("/sw.js");
		await navigator.serviceWorker.ready;
		if (!navigator.serviceWorker.controller) {
			await new Promise<void>((resolve) => {
				navigator.serviceWorker.addEventListener(
					"controllerchange",
					() => resolve(),
					{
						once: true,
					},
				);
			});
		}
	});
	await page.reload();
	await expect(page.getByRole("heading", { name: "佐藤 未来" })).toBeVisible();
});

test("demo does not send API requests to a live backend", async ({ page }) => {
	const liveApiRequests: string[] = [];
	await page.route("**/api/**", async (route) => {
		liveApiRequests.push(route.request().url());
		await route.abort();
	});

	await page.goto(`/demo/users/${userId}?demo=normal`);
	await expect(page.getByRole("heading", { name: "佐藤 未来" })).toBeVisible();
	await expect(page.locator("[data-demo-runtime='mock-only']")).toBeVisible();
	expect(liveApiRequests).toEqual([]);
});

test("demo recovers when a legacy root worker controls the page", async ({
	page,
}) => {
	await page.goto(`/demo/users/${userId}?demo=normal`);
	await page.evaluate(async () => {
		await navigator.serviceWorker.register("/__e2e_legacy_demo_worker__.js", {
			scope: "/",
		});
		await navigator.serviceWorker.ready;
		if (!navigator.serviceWorker.controller) {
			await new Promise<void>((resolve) => {
				navigator.serviceWorker.addEventListener(
					"controllerchange",
					() => resolve(),
					{
						once: true,
					},
				);
			});
		}
	});
	await page.reload();
	await expect(page.getByRole("heading", { name: "佐藤 未来" })).toBeVisible();
	await expect(page.locator("[data-demo-runtime='mock-only']")).toBeVisible();
});

test("demo static build supports nested direct load and refresh", async ({
	page,
}) => {
	await page.goto(`/demo/users/${userId}?demo=normal&d_profile=valid`);
	await expect(page.getByRole("heading", { name: "佐藤 未来" })).toBeVisible();
	await page.reload();
	await expect(page.getByRole("heading", { name: "佐藤 未来" })).toBeVisible();
});

test("demo opens the formal route family without authentication", async ({
	page,
}) => {
	for (const route of [
		["/demo/", "Dashboard"],
		["/demo/nodes", "Nodes"],
		["/demo/endpoints", "Endpoints"],
		["/demo/quota-policy", "Quota policy"],
		["/demo/service-config", "服务配置"],
		["/demo/tools", "Tools"],
		["/demo/system-status", "System status"],
	]) {
		await page.goto(`${route[0]}?demo=normal`);
		await expect(page).not.toHaveURL(/\/login/);
		await expect(
			page.getByRole("heading", { name: route[1], exact: true }),
		).toBeVisible();
	}
});

test("demo mutations update the formal page and redacted operation journal", async ({
	page,
}) => {
	await page.goto(`/demo/users/${userId}?demo=normal`);
	await page.getByRole("button", { name: "Reset token" }).click();
	const dialog = page.getByRole("alertdialog");
	await expect(dialog).toContainText("Reset subscription token");
	await dialog.getByRole("button", { name: "Reset" }).click();

	await expect(page.getByText(/Subscription token reset:/)).toBeVisible();
	await page.getByRole("button", { name: "Open Demo Inspector" }).click();
	await expect(
		page.getByRole("complementary", { name: "Demo Inspector" }),
	).toContainText("POST /api/admin/users/:id/reset-token · 200");
});

test("demo inspector collapses and recovers on mobile", async ({ page }) => {
	await page.setViewportSize({ width: 393, height: 852 });
	await page.goto(`/demo/users/${userId}?demo=normal`);
	await page.getByRole("button", { name: "Open Demo Inspector" }).click();
	await expect(
		page.getByRole("dialog").locator("h2:not(.sr-only)"),
	).toBeVisible();
	await page.getByRole("button", { name: "Collapse Inspector" }).click();
	await expect(
		page.getByRole("button", { name: "Open Demo Inspector" }),
	).toBeVisible();
});

test("demo inspector stays bounded across desktop layouts", async ({
	page,
}) => {
	await page.setViewportSize({ width: 1920, height: 900 });
	await page.goto(`/demo/users/${userId}?demo=normal`);
	await page.getByRole("button", { name: "Open Demo Inspector" }).click();

	const inspector = page.getByRole("complementary", { name: "Demo Inspector" });
	await expect(inspector).toBeVisible();
	const dockedBox = await inspector.boundingBox();
	expect(dockedBox).not.toBeNull();
	if (!dockedBox) throw new Error("Docked Inspector is not measurable");
	expect(dockedBox.x).toBeGreaterThanOrEqual(0);
	expect(dockedBox.y).toBeGreaterThanOrEqual(0);
	expect(dockedBox.x + dockedBox.width).toBeLessThanOrEqual(1920);
	expect(dockedBox.y + dockedBox.height).toBeLessThanOrEqual(900);

	await page.getByRole("button", { name: "Float panel" }).click();
	const floatingBox = await inspector.boundingBox();
	expect(floatingBox).not.toBeNull();
	if (!floatingBox) throw new Error("Floating Inspector is not measurable");
	expect(floatingBox.x).toBeGreaterThanOrEqual(0);
	expect(floatingBox.y).toBeGreaterThanOrEqual(0);
	expect(floatingBox.x + floatingBox.width).toBeLessThanOrEqual(1920);
	expect(floatingBox.y + floatingBox.height).toBeLessThanOrEqual(900);

	await page.getByRole("button", { name: "Collapse Inspector" }).click();
	await expect(
		page.getByRole("button", { name: "Open Demo Inspector" }),
	).toBeVisible();
});

test("demo control state survives client navigation and refresh", async ({
	page,
}) => {
	await page.goto(`/demo/users/${userId}?demo=normal`);
	await page.getByRole("button", { name: "Open Demo Inspector" }).click();
	let documentRequests = 0;
	page.on("request", (request) => {
		if (request.resourceType() === "document") documentRequests += 1;
	});

	await page.getByLabel("Scene").selectOption("large");
	await page.getByLabel("Network").selectOption("offline");
	await expect(page).toHaveURL(/demo=large/);
	await expect(page).toHaveURL(/d_network=offline/);
	expect(documentRequests).toBe(0);

	await page.reload();
	await expect(page.getByLabel("Scene")).toHaveValue("large");
	await expect(page.getByLabel("Network")).toHaveValue("offline");
});

test("demo share link reproduces controls in a fresh page", async ({
	page,
}) => {
	await page.goto(`/demo/users/${userId}?demo=normal`);
	await page.getByRole("button", { name: "Open Demo Inspector" }).click();
	await page.getByLabel("Scene").selectOption("large");
	await page.getByLabel("Network").selectOption("offline");
	await expect(page).toHaveURL(/demo=large/);
	await expect(page).toHaveURL(/d_network=offline/);
	await page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
	await page.getByRole("button", { name: "Copy demo link" }).click();

	const sharedUrl = await page.evaluate(() => navigator.clipboard.readText());
	const sharedPage = await page.context().newPage();
	await sharedPage.goto(sharedUrl);
	await expect(
		sharedPage.getByRole("complementary", { name: "Demo Inspector" }),
	).toBeVisible();
	await expect(sharedPage.getByLabel("Scene")).toHaveValue("large");
	await expect(sharedPage.getByLabel("Network")).toHaveValue("offline");
	await sharedPage.close();
});

test("demo identity switching stays on the same formal page", async ({
	page,
}) => {
	await page.goto(`/demo/users/${userId}?demo=normal&d_persona=anonymous`);

	await expect(
		page.getByRole("heading", { name: "Admin token required" }),
	).toBeVisible();
	await page.getByRole("button", { name: "Open Demo Inspector" }).click();
	await page.getByLabel("Persona / authentication").selectOption("admin");

	await expect(page.getByRole("heading", { name: "佐藤 未来" })).toBeVisible();
	await expect(page).toHaveURL(/d_persona=admin/);
});

test("demo Mihomo mirror walkthrough is URL-first and simulated", async ({
	page,
}) => {
	await page.goto(`/demo/users/${userId}?demo=normal&d_profile=valid`);

	const format = page.getByTestId("subscription-format");
	await page.getByText("Mihomo", { exact: true }).last().click();
	await page
		.getByRole("checkbox", {
			name: "Use XP mirror for external resources",
		})
		.check();
	await page.getByRole("button", { name: "Fetch" }).click();

	const dialog = page.getByRole("dialog");
	await expect(dialog).toContainText("XP mirror mode");
	await expect(dialog).toContainText("/api/mihomo/resources/demo-");
	await expect(page).toHaveURL(/d_format=mihomo/);
	await expect(page).toHaveURL(/d_mirror=mirror/);
});

test("demo Mihomo copy state follows the selected subscription mode", async ({
	page,
}) => {
	await page.goto(
		`/demo/users/${userId}?demo=normal&d_profile=valid&d_format=mihomo&d_mirror=mirror`,
	);
	await page.context().grantPermissions(["clipboard-read", "clipboard-write"]);

	const copyButton = page.getByRole("button", {
		name: "Copy subscription URL",
	});
	await copyButton.click();
	await expect(copyButton).toContainText("Copied");
	const copiedUrl = await page.evaluate(() => navigator.clipboard.readText());
	expect(copiedUrl).toContain("/api/sub/");
	expect(copiedUrl).toContain("format=mihomo");
	expect(copiedUrl).toContain("external_resources=mirror");

	await page.getByText("Raw", { exact: true }).click();
	await expect(
		page.getByRole("checkbox", {
			name: "Use XP mirror for external resources",
		}),
	).toHaveCount(0);
});

test("demo missing Mihomo profile is a structured simulated error", async ({
	page,
}) => {
	await page.goto(
		`/demo/users/${userId}?demo=normal&d_profile=missing&d_format=mihomo&d_mirror=1&d_preview=1`,
	);

	const dialog = page.getByRole("dialog");
	await expect(dialog).toBeVisible();
	await expect(dialog).toContainText("saved Mihomo profile");
});

test("demo invalid Mihomo profile rejects unsafe resources", async ({
	page,
}) => {
	await page.goto(
		`/demo/users/${userId}?demo=normal&d_profile=invalid&d_format=mihomo&d_mirror=1&d_preview=1`,
	);

	await expect(page.getByRole("dialog")).toContainText(
		"External resource URL must be HTTPS",
	);
});
