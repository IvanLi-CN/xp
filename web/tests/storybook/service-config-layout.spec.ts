import { expect, test } from "@playwright/test";

const LONG_ENVIRONMENT_NAME = "XP_MIHOMO_ALLOWED_PRIVATE_CIDRS";
const VIEWPORTS = [
	{ width: 320, height: 852 },
	{ width: 360, height: 800 },
	{ width: 393, height: 852 },
] as const;

function storyUrl(storyId: string) {
	return `/iframe.html?viewMode=story&id=${storyId}&globals=theme:light;density:comfortable`;
}

test.describe("Service config narrow layout", () => {
	for (const viewport of VIEWPORTS) {
		test(`${viewport.width}x${viewport.height} keeps the page within the viewport`, async ({
			page,
		}) => {
			await page.setViewportSize(viewport);
			await page.goto(storyUrl("pages-serviceconfigpage--provider-only"), {
				waitUntil: "networkidle",
			});

			const longEnvironmentName = page.getByText(LONG_ENVIRONMENT_NAME, {
				exact: true,
			});
			await expect(longEnvironmentName).toBeVisible();

			const metrics = await page.evaluate((environmentName) => {
				const element = Array.from(document.querySelectorAll("span")).find(
					(candidate) => candidate.textContent?.trim() === environmentName,
				);
				if (!element) throw new Error("Long environment name is missing");
				const paragraph = element.closest("p");
				if (!paragraph)
					throw new Error("Environment name paragraph is missing");
				const range = document.createRange();
				range.selectNodeContents(element);
				const lineTops = new Set(
					Array.from(range.getClientRects()).map((rect) =>
						Math.round(rect.top),
					),
				);
				const elementRect = element.getBoundingClientRect();
				const paragraphRect = paragraph.getBoundingClientRect();

				return {
					bodyScrollWidth: document.body.scrollWidth,
					documentScrollWidth: document.documentElement.scrollWidth,
					innerWidth: window.innerWidth,
					lineCount: lineTops.size,
					elementRight: elementRect.right,
					paragraphRight: paragraphRect.right,
				};
			}, LONG_ENVIRONMENT_NAME);

			expect(metrics.innerWidth).toBe(viewport.width);
			expect(metrics.bodyScrollWidth).toBeLessThanOrEqual(viewport.width);
			expect(metrics.documentScrollWidth).toBeLessThanOrEqual(viewport.width);
			expect(metrics.lineCount).toBeGreaterThan(0);
			expect(metrics.elementRight).toBeLessThanOrEqual(
				metrics.paragraphRight + 0.5,
			);
		});
	}
});
