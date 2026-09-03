import type { Meta, StoryObj } from "@storybook/react";
import { expect, userEvent, within } from "@storybook/test";

const meta = {
	title: "Pages/ServiceMonitorEditorPage",
	render: () => <div />,
	tags: ["autodocs", "coverage-ui", "service-monitor-editor"],
	parameters: {
		router: { initialEntry: "/monitors/new" },
		viewport: {
			defaultViewport: "serviceMonitorFormDesktop",
			viewports: {
				serviceMonitorFormDesktop: {
					name: "Service monitor form desktop (1280x900)",
					styles: { height: "900px", width: "1280px" },
					type: "desktop",
				},
				serviceMonitorFormMobile: {
					name: "Service monitor form mobile (393x852)",
					styles: { height: "852px", width: "393px" },
					type: "mobile",
				},
			},
		},
	},
} satisfies Meta;

export default meta;
type Story = StoryObj<typeof meta>;

export const NewMonitor: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			await canvas.findByRole("heading", { name: "New service monitor" }),
		).toBeInTheDocument();
		await expect(
			await canvas.findByRole("heading", { name: "Monitor configuration" }),
		).toBeInTheDocument();
		await expect(
			await canvas.findByRole("heading", { name: "Cluster test results" }),
		).toBeInTheDocument();
		await expect(
			await canvas.findByText(
				"Optional evidence from a staggered test across the frozen observer set. " +
					"It never blocks creation.",
			),
		).toBeInTheDocument();
		await userEvent.click(
			await canvas.findByRole("combobox", { name: "Method" }),
		);
		await userEvent.click(
			await within(document.body).findByRole("option", { name: "TCPING" }),
		);
		await expect(await canvas.findByLabelText("TCP port")).toBeInTheDocument();
	},
};

export const MobileTcping: Story = {
	parameters: {
		viewport: { defaultViewport: "serviceMonitorFormMobile" },
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(
			await canvas.findByRole("combobox", { name: "Method" }),
		);
		await userEvent.click(
			await within(document.body).findByRole("option", { name: "TCPING" }),
		);
		await expect(await canvas.findByLabelText("TCP port")).toBeInTheDocument();
	},
};

export const WideEvidence: Story = {
	parameters: {
		viewport: {
			defaultViewport: "serviceMonitorFormWide",
			viewports: {
				serviceMonitorFormWide: {
					name: "Service monitor form wide (1536x1000)",
					styles: { height: "1000px", width: "1536px" },
					type: "desktop",
				},
			},
		},
	},
};

export const MobileEvidence: Story = {
	parameters: {
		viewport: { defaultViewport: "serviceMonitorFormMobile" },
	},
};
