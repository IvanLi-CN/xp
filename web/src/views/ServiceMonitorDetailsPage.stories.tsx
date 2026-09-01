import type { Meta, StoryObj } from "@storybook/react";
import { expect, within } from "@storybook/test";

const meta = {
	title: "Pages/ServiceMonitorDetailsPage",
	render: () => <div />,
	tags: ["autodocs", "coverage-ui"],
	parameters: {
		router: { initialEntry: "/monitors/01JMONITOR00000000000000001" },
		viewport: {
			defaultViewport: "serviceMonitorDetailsDesktop",
			viewports: {
				serviceMonitorDetailsDesktop: {
					name: "Service monitor details desktop (1280x900)",
					styles: { height: "900px", width: "1280px" },
					type: "desktop",
				},
			},
		},
	},
} satisfies Meta;

export default meta;
type Story = StoryObj<typeof meta>;

export const CompleteHistory: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			await canvas.findByRole("heading", { name: "Public API health" }),
		).toBeInTheDocument();
		await expect(
			await canvas.findByText("Observer results"),
		).toBeInTheDocument();
	},
};
