import type { Meta, StoryObj } from "@storybook/react";
import { expect, userEvent, within } from "@storybook/test";

const NODE_ID = "node-hinet";

const meta = {
	title: "Pages/EndpointNewPage",
	render: () => <div />,
	parameters: {
		router: {
			initialEntry: "/endpoints/new",
		},
		mockApi: {
			data: {
				nodes: [
					{
						node_id: NODE_ID,
						node_name: "hinet",
						access_host: "hinet.example.com",
						api_base_url: "https://hinet-api.example.com:62416",
						quota_limit_bytes: 0,
						quota_reset: {
							policy: "monthly",
							day_of_month: 1,
							tz_offset_minutes: null,
						},
					},
				],
				realityDomains: [],
				endpoints: [],
			},
		},
	},
} satisfies Meta;

export default meta;

type Story = StoryObj<typeof meta>;

const expectSuggestionPanelToMatchInput = (input: HTMLElement) => {
	const panel = document.querySelector(
		'[data-testid="autocomplete-suggestions"]',
	);
	if (!(panel instanceof HTMLElement)) {
		throw new Error("Autocomplete suggestions panel was not rendered.");
	}
	const inputRect = input.getBoundingClientRect();
	const panelRect = panel.getBoundingClientRect();
	expect(Math.abs(panelRect.left - inputRect.left)).toBeLessThanOrEqual(2);
	expect(Math.abs(panelRect.width - inputRect.width)).toBeLessThanOrEqual(2);
};

export const ManualDestApiAddressSuggestion: Story = {};

export const ManualDestApiAddressSuggestionShowsOnEmptyFocus: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const destInput = await canvas.findByLabelText("dest");
		await expect(destInput).not.toHaveAttribute("list");
		await userEvent.click(destInput);
		await expect(
			await within(document.body).findByText("hinet-api.example.com:62416"),
		).toBeVisible();
		expectSuggestionPanelToMatchInput(destInput);
		await expect(
			within(document.body).queryByText("hinet API address"),
		).toBeNull();
		await expect(
			within(document.body).queryByText("Use this node's xp API origin."),
		).toBeNull();
		await userEvent.click(
			await within(document.body).findByText("hinet-api.example.com:62416"),
		);
		await expect(destInput).toHaveValue("hinet-api.example.com:62416");
	},
};
