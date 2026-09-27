import type { Meta, StoryObj } from "@storybook/react";
import { expect, within } from "@storybook/test";

import baseMeta from "./NodeDetailsPage.stories";

const meta = {
	...baseMeta,
	title: "Pages/NodeDetailsPage",
} satisfies Meta;

export default meta;

type Story = StoryObj<typeof meta>;

function runtimeActivityViewport(width: number) {
	const viewportName = `nodeActivity${width}`;
	return {
		viewport: {
			defaultViewport: viewportName,
			viewports: {
				[viewportName]: {
					name: `Node activity ${width}px`,
					styles: { width: `${width}px`, height: "852px" },
				},
			},
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
	parameters: runtimeActivityViewport(320),
	play: async ({ canvasElement }) => {
		await verifyRuntimeActivityResponsive(canvasElement);
	},
};

export const RuntimeActivityMobile360: Story = {
	parameters: runtimeActivityViewport(360),
	play: async ({ canvasElement }) => {
		await verifyRuntimeActivityResponsive(canvasElement);
	},
};

export const RuntimeActivityMobile393: Story = {
	parameters: runtimeActivityViewport(393),
	play: async ({ canvasElement }) => {
		await verifyRuntimeActivityResponsive(canvasElement);
	},
};

export const RuntimeActivityDesktop: Story = {
	parameters: runtimeActivityViewport(1280),
	play: async ({ canvasElement }) => {
		await verifyRuntimeActivityResponsive(canvasElement);
	},
};
