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
				endpoints: [
					{
						endpoint_id: "existing-managed-public-origin",
						node_id: NODE_ID,
						tag: "existing-managed-public-origin",
						kind: "vless_reality_vision_tcp",
						port: 443,
						meta: {
							managed_default: true,
							canary_upstream: {
								url: "https://node-xp.example.test",
								mode: "auto",
							},
						},
					},
					{
						endpoint_id: "existing-managed-upstream",
						node_id: NODE_ID,
						tag: "existing-managed-upstream",
						kind: "vless_reality_vision_tcp",
						port: 8443,
						meta: {
							managed_default: true,
							canary_upstream: {
								url: "http://127.0.0.1:8080",
								mode: "auto",
							},
						},
					},
				],
			},
		},
	},
} satisfies Meta;

export default meta;

type Story = StoryObj<typeof meta>;

export const ManagedDefaultFieldsVisible: Story = {
	tags: ["coverage-ui", "managed-vless-autocomplete"],
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			await canvas.findByRole("heading", { name: "New endpoint" }),
		).toBeInTheDocument();
		await expect(
			await canvas.findByLabelText("canaryUpstreamUrl"),
		).toBeInTheDocument();
		await expect(
			await canvas.findByLabelText("canary upstream mode"),
		).toBeInTheDocument();
		await expect(
			await canvas.findByLabelText("accepted host[:port]"),
		).toBeInTheDocument();
		await expect(
			await canvas.findByRole("button", {
				name: "Show upstream origin suggestions",
			}),
		).toBeInTheDocument();
		await expect(
			await canvas.findByRole("button", {
				name: "Show access host suggestions",
			}),
		).toBeInTheDocument();
		await expect(canvas.queryByLabelText("dest")).toBeNull();
		await expect(canvas.queryByLabelText("serverNames")).toBeNull();
	},
};

export const ManagedDefaultAutocompleteSuggestions: Story = {
	tags: ["coverage-ui", "managed-vless-autocomplete"],
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(
			await canvas.findByRole("button", {
				name: "Show upstream origin suggestions",
			}),
		);
		await expect(
			within(document.body).queryByText("https://node-xp.example.test"),
		).toBeNull();
		await userEvent.click(
			await within(
				await within(document.body).findByTestId("autocomplete-suggestions"),
			).findByText("http://127.0.0.1:8080"),
		);
		await expect(await canvas.findByLabelText("canaryUpstreamUrl")).toHaveValue(
			"http://127.0.0.1:8080",
		);

		await userEvent.click(
			await canvas.findByRole("button", {
				name: "Show access host suggestions",
			}),
		);
		await userEvent.click(
			await within(
				await within(document.body).findByTestId("tag-input-suggestions"),
			).findByText("node-xp.example.test"),
		);
		await expect(
			await canvas.findByTitle("node-xp.example.test:443"),
		).toBeInTheDocument();
	},
};

export const ManagedDefaultAcceptedHostDefaultsTo443: Story = {
	tags: ["coverage-ui", "managed-vless-autocomplete"],
	parameters: {
		mockApi: {
			data: {
				nodes: [
					{
						node_id: NODE_ID,
						node_name: "alpha",
						access_host: "",
						api_base_url: "not-a-url",
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
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const input = await canvas.findByLabelText("accepted host[:port]");
		const tagInputControl = input.closest("[data-testid='tag-input-control']");
		if (!(tagInputControl instanceof HTMLElement)) {
			throw new Error("accepted host tag input control not found");
		}
		await userEvent.type(input, "edge.example.com");
		await userEvent.click(
			await within(tagInputControl).findByRole("button", { name: "Add" }),
		);
		await expect(
			await within(tagInputControl).findByTitle("edge.example.com:443"),
		).toBeInTheDocument();
	},
};
