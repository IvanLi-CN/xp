import type { Meta, StoryObj } from "@storybook/react";
import { expect, fireEvent, within } from "@storybook/test";

const meta = {
	title: "Pages/ToolsPage",
	render: () => <div />,
	parameters: {
		router: {
			initialEntry: "/tools",
		},
	},
} satisfies Meta;

export default meta;

type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const WithPreview: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);

		fireEvent.change(await canvas.findByLabelText("Source text"), {
			target: {
				value: "server: edge.example.com\npassword: super-secret\n",
			},
		});
		fireEvent.click(await canvas.findByRole("button", { name: "Run redact" }));

		await expect(await canvas.findByLabelText("Redacted result")).toHaveValue(
			"server: e***.example.com\npassword: supe***cret\n",
		);
	},
};
