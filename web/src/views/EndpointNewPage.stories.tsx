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
				name: "Show XP HTTPS listener suggestions",
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

export const ManagedDefaultAutocompleteSuggestions: Story = {
	tags: ["coverage-ui", "managed-vless-autocomplete"],
	parameters: {
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
						endpoint_id: "endpoint-existing",
						node_id: NODE_ID,
						tag: "managed-alpha",
						kind: "vless_reality_vision_tcp",
						port: 443,
						meta: {
							reality: {
								dest: "127.0.0.1:49043",
								server_names: ["node-xp.example.test"],
								server_names_source: "manual",
								fingerprint: "chrome",
							},
							managed_default: true,
						},
					},
				],
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.clear(await canvas.findByLabelText("port"));
		await userEvent.type(await canvas.findByLabelText("port"), "8443");

		await userEvent.click(
			await canvas.findByRole("button", {
				name: "Show XP HTTPS listener suggestions",
			}),
		);
		const suggestionPanel = await within(document.body).findByTestId(
			"autocomplete-suggestions",
		);
		await expect(
			within(suggestionPanel)
				.getAllByText(/^https:\/\/127\.0\.0\.1:/)
				.map((element) => element.textContent),
		).toEqual(["https://127.0.0.1:49043", "https://127.0.0.1:39043"]);
		await userEvent.click(
			await within(suggestionPanel).findByText("https://127.0.0.1:49043"),
		);
		await expect(await canvas.findByLabelText("canaryUpstreamUrl")).toHaveValue(
			"https://127.0.0.1:49043",
		);

		await userEvent.click(
			await canvas.findByRole("button", {
				name: "Show access host suggestions",
			}),
		);
		await userEvent.click(
			await within(
				await within(document.body).findByTestId("tag-input-suggestions"),
			).findByText("node-xp.example.test:8443"),
		);
		await expect(
			await canvas.findByTitle("node-xp.example.test:8443"),
		).toBeInTheDocument();
	},
};

export const ManagedDefaultNodeAliasSuggestionsWithoutUpstreamHistory: Story = {
	tags: ["coverage-ui", "managed-vless-autocomplete"],
	parameters: {
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
				endpoints: [],
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
		await expect(await canvas.findByLabelText("canaryUpstreamUrl")).toHaveValue(
			"https://127.0.0.1:39043",
		);

		await userEvent.click(
			await canvas.findByRole("button", {
				name: "Show access host suggestions",
			}),
		);
		await userEvent.click(
			await within(
				await within(document.body).findByTestId("tag-input-suggestions"),
			).findByText("hinet-ep.707979.xyz"),
		);
		await expect(
			await canvas.findByTitle("hinet-ep.707979.xyz:443"),
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
