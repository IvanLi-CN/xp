import type { Meta, StoryObj } from "@storybook/react";
import { expect, userEvent, within } from "@storybook/test";

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
		const originalEditor = canvasElement.querySelector<HTMLElement>(
			".cm-merge-a .cm-content[contenteditable='true']",
		);

		if (!originalEditor) {
			throw new Error("Unable to find the editable Mihomo source pane.");
		}

		await userEvent.click(originalEditor);
		await userEvent.keyboard(
			"server: edge.example.com{Enter}password: super-secret{Enter}",
		);
		await userEvent.click(
			await canvas.findByRole("button", { name: "Run redact" }),
		);

		const modifiedEditor = canvasElement.querySelector<HTMLElement>(
			".cm-merge-b .cm-content",
		);

		await expect(modifiedEditor).toHaveTextContent(
			"server: e***.example.compassword: supe***cret",
		);
	},
};
