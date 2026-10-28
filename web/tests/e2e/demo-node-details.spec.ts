import { expect, test } from "@playwright/test";
import { fixtureCatalog } from "../../src/fixture-policy/catalog";

test("demo preserves node traffic tab and share controls while switching nodes", async ({
	page,
}) => {
	const firstNodeId = fixtureCatalog.nodeId.fixture32();
	const secondNodeId = fixtureCatalog.nodeId.fixture36();
	await page.goto(`/demo/nodes/${firstNodeId}?demo=normal&tab=traffic`);

	const trafficPanel = page.getByRole("tabpanel", { name: "Traffic" });
	await expect(trafficPanel).toBeVisible();
	await expect(page.getByText("Last sample:")).toBeVisible();
	await expect(trafficPanel).toContainText("5 GiB");
	await expect(trafficPanel).toContainText("10 GiB");
	const firstTrafficText = await trafficPanel.innerText();
	const secondNodeLink = page.getByRole("link", {
		name: `Node ${fixtureCatalog.nodeName.fixture37()} (${secondNodeId})`,
	});
	await secondNodeLink.click();

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
	await expect(trafficPanel).toBeVisible();
	await expect(page.getByText("Last sample:")).toBeVisible();
	await expect(trafficPanel).toContainText("4 GiB");
	await expect(trafficPanel).toContainText("11 GiB");
	await expect(trafficPanel.innerText()).not.toEqual(firstTrafficText);
	await expect(
		page.getByText(
			"Traffic data is partial. Missing samples remain gaps in the chart.",
			{ exact: true },
		),
	).toHaveCount(0);
	await expect(secondNodeLink).toHaveAttribute("aria-current", "page");
	await expect(secondNodeLink).toHaveClass(/bg-primary\/10/);
});
