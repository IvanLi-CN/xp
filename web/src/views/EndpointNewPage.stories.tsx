import type { Meta, StoryObj } from "@storybook/react";
import { expect, userEvent, within } from "@storybook/test";

const NODE_ID = "node-alpha";

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
						node_name: "alpha",
						access_host: "node-xp.example.test",
						api_base_url: "https://node-xp.example.test:443",
						quota_limit_bytes: 0,
						quota_reset: {
							policy: "monthly",
							day_of_month: 1,
							tz_offset_minutes: null,
						},
					},
				],
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

const expectTagSuggestionPanelToMatchControl = () => {
	const control = document.querySelector('[data-testid="tag-input-control"]');
	const panel = document.querySelector('[data-testid="tag-input-suggestions"]');
	if (!(control instanceof HTMLElement)) {
		throw new Error("Tag input control was not rendered.");
	}
	if (!(panel instanceof HTMLElement)) {
		throw new Error("Tag input suggestions panel was not rendered.");
	}
	const controlRect = control.getBoundingClientRect();
	const panelRect = panel.getBoundingClientRect();
	expect(Math.abs(panelRect.left - controlRect.left)).toBeLessThanOrEqual(2);
	expect(Math.abs(panelRect.width - controlRect.width)).toBeLessThanOrEqual(2);
};

export const ManualDestApiAddressSuggestion: Story = {};

export const ManualDestApiAddressSuggestionShowsOnEmptyFocus: Story = {
	tags: ["coverage-ui"],
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const destInput = await canvas.findByLabelText("dest");
		await expect(canvas.queryByText("node-xp.example.test:443")).toBeNull();
		await expect(destInput).not.toHaveAttribute("list");
		await userEvent.click(destInput);
		await expect(
			await within(document.body).findByText("127.0.0.1:62416"),
		).toBeVisible();
		expectSuggestionPanelToMatchInput(destInput);
		const destPanel = document.querySelector(
			'[data-testid="autocomplete-suggestions"]',
		);
		if (!(destPanel instanceof HTMLElement)) {
			throw new Error("Autocomplete suggestions panel was not rendered.");
		}
		await expect(
			within(destPanel).queryByText("node-xp.example.test:443"),
		).toBeNull();
		await expect(
			within(document.body).queryByText("node-beta API address"),
		).toBeNull();
		await expect(
			within(document.body).queryByText("Use this node's xp API origin."),
		).toBeNull();
		await userEvent.click(
			await within(document.body).findByText("127.0.0.1:62416"),
		);
		await expect(destInput).toHaveValue("127.0.0.1:62416");
		await userEvent.clear(destInput);
		await userEvent.type(destInput, "origin.example.test:443");
		await userEvent.click(await canvas.findByLabelText("serverNames"));
		await expect(
			await within(document.body).findByText("origin.example.test:443"),
		).toBeVisible();
		expectTagSuggestionPanelToMatchControl();
		await expect(
			within(document.body).queryByText("node-xp.example.test:443"),
		).toBeNull();
	},
};
