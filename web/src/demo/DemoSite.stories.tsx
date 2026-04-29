import type { Meta, StoryObj } from "@storybook/react";
import { expect, userEvent, within } from "@storybook/test";

function Empty() {
	return <></>;
}

const meta = {
	title: "Demo/Site",
	component: Empty,
	parameters: {
		layout: "fullscreen",
		router: {
			initialEntry: "/demo/login",
		},
	},
} satisfies Meta<typeof Empty>;

export default meta;

type Story = StoryObj<typeof meta>;

export const MainFlow: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);

		await expect(
			await canvas.findByRole("heading", { name: "xp Demo Site" }),
		).toBeInTheDocument();

		await userEvent.click(
			await canvas.findByRole("button", { name: "Enter demo" }),
		);

		await expect(
			await canvas.findByRole("heading", { name: "Demo dashboard" }),
		).toBeInTheDocument();
		await expect(
			await canvas.findByText(
				"231 GiB finite used / 420 GiB, 1 unlimited user",
			),
		).toBeInTheDocument();

		await userEvent.click(
			await canvas.findByRole("link", { name: "New endpoint" }),
		);
		await expect(
			await canvas.findByRole("heading", { name: "New endpoint" }),
		).toBeInTheDocument();
		await userEvent.click(
			await canvas.findByRole("button", { name: "Create endpoint" }),
		);
		await expect(
			await canvas.findByText(/endpoint-demo-01/),
		).toBeInTheDocument();

		await userEvent.click(await canvas.findByRole("link", { name: "Users" }));
		await expect(
			await canvas.findByRole("heading", { name: "Users" }),
		).toBeInTheDocument();
		await userEvent.type(await canvas.findByLabelText("Search users"), "sato");
		await expect(await canvas.findByText("佐藤 未来")).toBeInTheDocument();

		await userEvent.click(
			await canvas.findByRole("link", { name: "Quota policy" }),
		);
		await expect(
			await canvas.findByRole("heading", { name: "Quota policy" }),
		).toBeInTheDocument();

		await userEvent.click(
			await canvas.findByRole("link", { name: "Reality domains" }),
		);
		await expect(
			await canvas.findByRole("heading", { name: "Reality domains" }),
		).toBeInTheDocument();

		await userEvent.click(
			await canvas.findByRole("link", { name: "Service config" }),
		);
		await expect(
			await canvas.findByRole("heading", { name: "Service config" }),
		).toBeInTheDocument();

		await userEvent.click(await canvas.findByRole("link", { name: "Tools" }));
		await expect(
			await canvas.findByRole("heading", { name: "Tools" }),
		).toBeInTheDocument();
		await userEvent.click(
			await canvas.findByRole("button", { name: "Run redaction" }),
		);
		await expect(await canvas.findByText(/\[REDACTED\]/)).toBeInTheDocument();

		await userEvent.click(
			await canvas.findByRole("link", { name: "Endpoints" }),
		);
		await userEvent.click(await canvas.findByRole("link", { name: /tokyo/i }));
		await userEvent.click(
			await canvas.findByRole("link", { name: "Probe stats" }),
		);
		await expect(
			await canvas.findByRole("heading", { name: "Endpoint probe stats" }),
		).toBeInTheDocument();
	},
};
