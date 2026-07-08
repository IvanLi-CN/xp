import type { Meta, StoryObj } from "@storybook/react";
import { expect, userEvent, within } from "@storybook/test";

const NODE_ID = "node-1";
const ENDPOINT_ID = "endpoint-managed-vless";

const meta = {
	title: "Pages/EndpointDetailsPage",
	render: () => <div />,
	parameters: {
		router: {
			initialEntry: `/endpoints/${ENDPOINT_ID}`,
		},
		mockApi: {
			data: {
				nodes: [
					{
						node_id: NODE_ID,
						node_name: "tokyo-1",
						access_host: "edge.example.test",
						api_base_url: "https://tokyo-1.example.com",
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
						endpoint_id: ENDPOINT_ID,
						node_id: NODE_ID,
						tag: "managed-vless",
						kind: "vless_reality_vision_tcp",
						port: 53844,
						meta: {
							reality: {
								dest: "127.0.0.1:39043",
								server_names: ["edge.example.test"],
								server_names_source: "manual",
								fingerprint: "chrome",
							},
							managed_default: true,
							canary_upstream: {
								url: "http://127.0.0.1:8080",
								mode: "auto",
							},
							accepted_authorities: [
								"endpoint.example.test:53844",
								"edge.example.com:53844",
							],
						},
						short_ids: ["2a3b4c"],
						active_short_id: "2a3b4c",
					},
					{
						endpoint_id: "endpoint-managed-public-origin",
						node_id: NODE_ID,
						tag: "managed-public-origin",
						kind: "vless_reality_vision_tcp",
						port: 443,
						meta: {
							reality: {
								dest: "127.0.0.1:39043",
								server_names: ["edge.example.test"],
								server_names_source: "manual",
								fingerprint: "chrome",
							},
							managed_default: true,
							canary_upstream: {
								url: "https://tokyo-1.example.com",
								mode: "auto",
							},
							accepted_authorities: [],
						},
						short_ids: ["c1d2e3"],
						active_short_id: "c1d2e3",
					},
					{
						endpoint_id: "endpoint-managed-sibling",
						node_id: NODE_ID,
						tag: "managed-sibling",
						kind: "vless_reality_vision_tcp",
						port: 9443,
						meta: {
							reality: {
								dest: "127.0.0.1:39043",
								server_names: ["edge.example.test"],
								server_names_source: "manual",
								fingerprint: "chrome",
							},
							managed_default: true,
							canary_upstream: {
								url: "http://127.0.0.1:9000",
								mode: "auto",
							},
							accepted_authorities: ["edge.example.test:443"],
						},
						short_ids: ["7f8e9d"],
						active_short_id: "7f8e9d",
					},
				],
			},
		},
	},
} satisfies Meta;

export default meta;

type Story = StoryObj<typeof meta>;

export const ManagedDefaultAliases: Story = {
	tags: ["managed-vless-autocomplete"],
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const backAction = await canvas.findByRole("link", { name: "Back" });
		const refreshAction = await canvas.findByRole("button", {
			name: "Refresh",
		});
		await expect(
			await canvas.findByRole("heading", { name: "Endpoint details" }),
		).toBeInTheDocument();
		await expect(
			Math.abs(
				backAction.getBoundingClientRect().height -
					refreshAction.getBoundingClientRect().height,
			),
		).toBeLessThanOrEqual(1);
		await expect(
			await canvas.findByText("acceptedAuthorities"),
		).toBeInTheDocument();
		await expect(
			await canvas.findAllByText("endpoint.example.test:53844"),
		).toHaveLength(2);
		await expect(
			await canvas.findAllByText("edge.example.com:53844"),
		).toHaveLength(2);
		await expect(
			await canvas.findByText(
				"Accept additional ordinary HTTPS Host headers for camouflage routing. Omit port to use HTTPS default 443. This does not change REALITY serverNames or the canonical /generate_204 URL.",
			),
		).toBeInTheDocument();
	},
};

export const ManagedDefaultAliasDefaultsTo443: Story = {
	tags: ["managed-vless-autocomplete"],
	parameters: {
		mockApi: {
			data: {
				nodes: [
					{
						node_id: NODE_ID,
						node_name: "tokyo-1",
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
				endpoints: [
					{
						endpoint_id: ENDPOINT_ID,
						node_id: NODE_ID,
						tag: "managed-vless",
						kind: "vless_reality_vision_tcp",
						port: 53844,
						meta: {
							reality: {
								dest: "127.0.0.1:39043",
								server_names: ["edge.example.test"],
								server_names_source: "manual",
								fingerprint: "chrome",
							},
							managed_default: true,
							canary_upstream: {
								url: "http://127.0.0.1:8080",
								mode: "auto",
							},
							accepted_authorities: [
								"endpoint.example.test:53844",
								"edge.example.com:53844",
							],
						},
						short_ids: ["2a3b4c"],
						active_short_id: "2a3b4c",
					},
				],
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			await canvas.findByRole("heading", { name: "Endpoint details" }),
		).toBeInTheDocument();
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

export const ManagedDefaultAutocompleteSuggestions: Story = {
	tags: ["managed-vless-autocomplete"],
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
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

		await userEvent.click(
			await canvas.findByRole("button", {
				name: "Show upstream origin suggestions",
			}),
		);
		await expect(
			within(document.body).queryByText("https://tokyo-1.example.com"),
		).toBeNull();
		await userEvent.click(
			await within(
				await within(document.body).findByTestId("autocomplete-suggestions"),
			).findByText("http://127.0.0.1:9000"),
		);
		await expect(
			await canvas.findByLabelText("canary upstream url"),
		).toHaveValue("http://127.0.0.1:9000");

		await userEvent.click(
			await canvas.findByRole("button", {
				name: "Show access host suggestions",
			}),
		);
		await userEvent.click(
			await within(
				await within(document.body).findByTestId("tag-input-suggestions"),
			).findByText("edge.example.test"),
		);
		await expect(
			await canvas.findByTitle("edge.example.test:443"),
		).toBeInTheDocument();
	},
};
