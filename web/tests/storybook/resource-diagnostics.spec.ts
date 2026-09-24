import { expect, test } from "@playwright/test";

function storyUrl(storyId: string, globals = "theme:dark;density:comfortable") {
	return `/iframe.html?viewMode=story&id=${storyId}&globals=${globals}`;
}

test("keeps circuit-open retry disabled on the Node details mobile state", async ({
	page,
}) => {
	await page.setViewportSize({ width: 393, height: 852 });
	await page.goto(storyUrl("pages-nodedetailspage--resources-circuit-open"), {
		waitUntil: "networkidle",
	});

	await expect(page.getByText("Peer path is cooling down")).toBeVisible({
		timeout: 15_000,
	});
	await expect(
		page.getByRole("button", { name: /Retry resource read/i }),
	).toBeDisabled();
	await expect(
		page.getByText(/No new resource request was sent/),
	).toBeVisible();
});

test("renders light-theme circuit-open state safely", async ({ page }) => {
	await page.setViewportSize({ width: 393, height: 852 });
	await page.goto(
		storyUrl(
			"pages-nodedetailspage--resources-circuit-open",
			"theme:light;density:comfortable",
		),
		{ waitUntil: "networkidle" },
	);
	await expect(page.getByText("Peer path is cooling down")).toBeVisible({
		timeout: 15_000,
	});
	await expect(
		page.getByRole("button", { name: /Retry resource read/i }),
	).toBeDisabled();
});

test("keeps stale snapshot age and history errors visible", async ({
	page,
}) => {
	await page.goto(
		storyUrl("components-resourcesnapshotpanel--stale-snapshot"),
		{ waitUntil: "networkidle" },
	);
	await expect(
		page.getByText(/Showing the last successful snapshot/),
	).toBeVisible({ timeout: 15_000 });
	await expect(page.getByText(/age \d+ min|age \d+ hr/)).toBeVisible({
		timeout: 15_000,
	});

	await page.goto(
		storyUrl(
			"components-resourcesnapshotpanel--history-refresh-error",
			"theme:light;density:comfortable",
		),
		{ waitUntil: "networkidle" },
	);
	await expect(
		page.getByText("Refresh failed; showing the last successful points."),
	).toBeVisible({
		timeout: 15_000,
	});
	await expect(
		page.getByRole("button", { name: "Retry history" }),
	).toBeVisible();
});

test("renders light-theme stale snapshot and history error states", async ({
	page,
}) => {
	await page.goto(
		storyUrl(
			"components-resourcesnapshotpanel--stale-snapshot",
			"theme:light;density:comfortable",
		),
		{ waitUntil: "networkidle" },
	);
	await expect(
		page.getByText("Showing the last successful snapshot."),
	).toBeVisible({ timeout: 15_000 });
	await page.goto(
		storyUrl(
			"components-resourcesnapshotpanel--history-refresh-error",
			"theme:light;density:comfortable",
		),
		{ waitUntil: "networkidle" },
	);
	await expect(
		page.getByText("Refresh failed; showing the last successful points."),
	).toBeVisible({ timeout: 15_000 });
});

test("keeps unstructured resource failures safe and actionable", async ({
	page,
}) => {
	await page.goto(
		storyUrl(
			"components-resourcesnapshotpanel--unknown-error",
			"theme:light;density:comfortable",
		),
		{ waitUntil: "networkidle" },
	);
	await expect(page.getByText("Resource read failed")).toBeVisible({
		timeout: 15_000,
	});
	await expect(
		page.getByText("unstructured backend detail must stay hidden"),
	).toHaveCount(0);
	await expect(
		page.getByRole("button", { name: "Retry resource read" }),
	).toBeVisible();
});

test("renders light-theme unknown failure state safely", async ({ page }) => {
	await page.goto(
		storyUrl(
			"components-resourcesnapshotpanel--unknown-error",
			"theme:light;density:comfortable",
		),
		{ waitUntil: "networkidle" },
	);
	await expect(page.getByText("Resource read failed")).toBeVisible({
		timeout: 15_000,
	});
	await expect(
		page.getByText("unstructured backend detail must stay hidden"),
	).toHaveCount(0);
});
