import type { Meta, StoryObj } from "@storybook/react";
import { expect, userEvent, within } from "@storybook/test";

const meta = {
	title: "Pages/ServiceMonitorsPage",
	render: () => <div />,
	tags: ["autodocs", "coverage-ui"],
	parameters: {
		router: { initialEntry: "/monitors" },
		viewport: {
			defaultViewport: "serviceMonitoringDesktop",
			viewports: {
				serviceMonitoringDesktop: {
					name: "Service monitoring desktop (1280x900)",
					styles: { height: "900px", width: "1280px" },
					type: "desktop",
				},
			},
		},
	},
} satisfies Meta;

export default meta;
type Story = StoryObj<typeof meta>;

export const MonitoringWorkspace: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const refresh = await canvas.findByRole("button", { name: "Refresh" });
		const create = await canvas.findByRole("link", { name: "New monitor" });

		await expect(Math.round(refresh.getBoundingClientRect().width)).toBe(
			Math.round(create.getBoundingClientRect().width),
		);
		await expect(Math.round(refresh.getBoundingClientRect().height)).toBe(
			Math.round(create.getBoundingClientRect().height),
		);
	},
};

export const Overview: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			await canvas.findByRole("heading", { name: "Service monitoring" }),
		).toBeInTheDocument();
		await userEvent.click(
			await canvas.findByRole("link", { name: /Public API health/ }),
		);
		await expect(
			await canvas.findByRole("heading", { name: "Public API health" }),
		).toBeInTheDocument();
	},
};
