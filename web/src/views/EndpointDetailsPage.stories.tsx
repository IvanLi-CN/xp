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

export const MihomoSmuxDefaults: Story = {
	tags: ["coverage-ui", "endpoint-mihomo-smux"],
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(await canvas.findByText("高级设置：连接复用 (SMux)"));
		await expect(await canvas.findByLabelText("启用 SMux")).toBeChecked();
		await expect(await canvas.findByLabelText("最大物理连接数")).toHaveValue(4);
		await expect(
			await canvas.findByText(/Mihomo >= v1.19.29/),
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
				name: "Show XP HTTPS listener suggestions",
			}),
		).toBeInTheDocument();
		await expect(
			await canvas.findByRole("button", {
				name: "Show access host suggestions",
			}),
		).toBeInTheDocument();

		await userEvent.click(
			await canvas.findByRole("button", {
				name: "Show XP HTTPS listener suggestions",
			}),
		);
		await userEvent.click(
			await within(
				await within(document.body).findByTestId("autocomplete-suggestions"),
			).findByText("https://127.0.0.1:39043"),
		);
		await expect(
			await canvas.findByLabelText("canary upstream url"),
		).toHaveValue("https://127.0.0.1:39043");

		await userEvent.click(
			await canvas.findByRole("button", {
				name: "Show access host suggestions",
			}),
		);
		await userEvent.click(
			await within(
				await within(document.body).findByTestId("tag-input-suggestions"),
			).findByText("edge.example.test:53844"),
		);
		await expect(
			await canvas.findByTitle("edge.example.test:53844"),
		).toBeInTheDocument();
	},
};

export const ManagedDefaultNodeAliasSuggestionsWithoutUpstreamHistory: Story = {
	tags: ["managed-vless-autocomplete"],
	parameters: {
		router: {
			initialEntry: "/endpoints/endpoint-hinet-managed",
		},
		mockApi: {
			data: {
				nodes: [
					{
						node_id: "node-hinet",
						node_name: "hinet",
						access_host: "hinet-ep.707979.xyz",
						api_base_url: "https://hinet-xp.707979.xyz",
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
						endpoint_id: "endpoint-hinet-managed",
						node_id: "node-hinet",
						tag: "managed-hinet",
						kind: "vless_reality_vision_tcp",
						port: 53844,
						meta: {
							reality: {
								dest: "127.0.0.1:39043",
								server_names: ["hinet-ep.707979.xyz"],
								server_names_source: "manual",
								fingerprint: "chrome",
							},
							managed_default: true,
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
		await userEvent.click(
			await canvas.findByRole("button", {
				name: "Show XP HTTPS listener suggestions",
			}),
		);
		await userEvent.click(
			await within(
				await within(document.body).findByTestId("autocomplete-suggestions"),
			).findByText("https://127.0.0.1:39043"),
		);
		await expect(
			await canvas.findByLabelText("canary upstream url"),
		).toHaveValue("https://127.0.0.1:39043");

		await userEvent.click(
			await canvas.findByRole("button", {
				name: "Show access host suggestions",
			}),
		);
		await userEvent.click(
			await within(
				await within(document.body).findByTestId("tag-input-suggestions"),
			).findByText("hinet-ep.707979.xyz:53844"),
		);
		await expect(
			await canvas.findByTitle("hinet-ep.707979.xyz:53844"),
		).toBeInTheDocument();
	},
};
