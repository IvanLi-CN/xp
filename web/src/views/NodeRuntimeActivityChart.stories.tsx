import type { Meta, StoryObj } from "@storybook/react";
import { expect, within } from "@storybook/test";

import baseMeta from "./NodeDetailsPage.stories";

const meta = {
	...baseMeta,
	title: "Pages/NodeDetailsPage",
} satisfies Meta;

export default meta;

type Story = StoryObj<typeof meta>;

const runtimeActivityViewports = {
	nodeActivity320: {
		name: "Node activity 320px",
		styles: { width: "320px", height: "852px" },
	},
	nodeActivity360: {
		name: "Node activity 360px",
		styles: { width: "360px", height: "852px" },
	},
	nodeActivity393: {
		name: "Node activity 393px",
		styles: { width: "393px", height: "852px" },
	},
	nodeActivity1280: {
		name: "Node activity 1280px",
		styles: { width: "1280px", height: "852px" },
	},
};

function runtimeActivityViewport(
	viewportName: keyof typeof runtimeActivityViewports,
) {
	return {
		viewport: {
			defaultViewport: viewportName,
			viewports: runtimeActivityViewports,
		},
	};
}

async function verifyRuntimeActivityResponsive(canvasElement: HTMLElement) {
	const canvas = within(canvasElement);
	const chart = await canvas.findByRole("region", {
		name: "7-day service activity chart",
	});
	await expect(chart).toHaveAttribute(
		"aria-label",
		"7-day service activity chart",
	);
	await expect(chart.firstElementChild).toHaveClass("min-w-[28rem]");
	await expect(await canvas.findByText("00:00")).toBeInTheDocument();
	await expect(await canvas.findByText("24:00")).toBeInTheDocument();
	expect(canvasElement.scrollWidth).toBeLessThanOrEqual(
		canvasElement.clientWidth,
	);
	expect(
		canvasElement.querySelector(
			'button[aria-label="Scroll activity chart left"]',
		),
	).not.toBeNull();
	expect(
		canvasElement.querySelector(
			'button[aria-label="Scroll activity chart right"]',
		),
	).not.toBeNull();
}

export const RuntimeActivityMobile320: Story = {
	parameters: runtimeActivityViewport("nodeActivity320"),
	play: async ({ canvasElement }) => {
		await verifyRuntimeActivityResponsive(canvasElement);
	},
};

export const RuntimeActivityMobile360: Story = {
	parameters: runtimeActivityViewport("nodeActivity360"),
	play: async ({ canvasElement }) => {
		await verifyRuntimeActivityResponsive(canvasElement);
	},
};

export const RuntimeActivityMobile393: Story = {
	parameters: runtimeActivityViewport("nodeActivity393"),
	play: async ({ canvasElement }) => {
		await verifyRuntimeActivityResponsive(canvasElement);
	},
};

export const RuntimeActivityDesktop: Story = {
	parameters: runtimeActivityViewport("nodeActivity1280"),
	play: async ({ canvasElement }) => {
		await verifyRuntimeActivityResponsive(canvasElement);
	},
};
