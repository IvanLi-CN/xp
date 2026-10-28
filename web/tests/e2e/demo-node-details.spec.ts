import { expect, test } from "@playwright/test";
import { fixtureCatalog } from "../../src/fixture-policy/catalog";

test("demo preserves node traffic tab and share controls while switching nodes", async ({
	page,
}) => {
	const firstNodeId = fixtureCatalog.nodeId.fixture32();
	const secondNodeId = fixtureCatalog.nodeId.fixture36();
	await page.goto(`/demo/nodes/${firstNodeId}?demo=normal&tab=traffic`);

	await expect(page.getByRole("tabpanel", { name: "Traffic" })).toBeVisible();
	await expect(page.getByText("Last sample:")).toBeVisible();
	await page
		.getByRole("link", {
			name: `Node ${fixtureCatalog.nodeName.fixture37()} (${secondNodeId})`,
		})
		.click();

	await expect
		.poll(() => {
			const url = new URL(page.url());
			return {
				pathname: url.pathname,
				demo: url.searchParams.get("demo"),
				tab: url.searchParams.get("tab"),
			};
		})
		.toEqual({
			pathname: `/demo/nodes/${secondNodeId}`,
			demo: "normal",
			tab: "traffic",
		});
	await expect(page.getByRole("tabpanel", { name: "Traffic" })).toBeVisible();
	await expect(page.getByText("Last sample:")).toBeVisible();
});
