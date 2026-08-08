import type { Meta, StoryObj } from "@storybook/react";
import { expect, userEvent, within } from "@storybook/test";
import { fixtureCatalog } from "../fixture-policy/catalog";

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
						node_id: fixtureCatalog.slotString.s118(),
						node_name: fixtureCatalog.slotString.s33(),
						access_host: fixtureCatalog.slotString.s119(),
						api_base_url: fixtureCatalog.slotString.s34(),
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
						endpoint_id: fixtureCatalog.slotString.s120(),
						node_id: fixtureCatalog.slotString.s118(),
						tag: fixtureCatalog.slotString.s121(),
						kind: "vless_reality_vision_tcp",
						port: 53844,
						meta: {
							reality: {
								dest: fixtureCatalog.slotString.s2(),
								server_names: fixtureCatalog.slotList.l5(),
								server_names_source: "manual",
								fingerprint: "chrome",
							},
							managed_default: true,
							canary_upstream: fixtureCatalog.slotString.s122(),
							accepted_authorities: fixtureCatalog.slotList.l6(),
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
			await canvas.findAllByText(fixtureCatalog.slotList.l6()[0]),
		).toHaveLength(2);
		await expect(
			await canvas.findAllByText(fixtureCatalog.slotList.l6()[1]),
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
	parameters: {
		mockApi: {
			data: {
				endpoints: [
					{
						endpoint_id: ENDPOINT_ID,
						node_id: NODE_ID,
						tag: "legacy-ss2022",
						kind: "ss2022_2022_blake3_aes_128_gcm",
						port: 443,
						meta: {},
					},
				],
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(
			await canvas.findByText("高级设置：SS2022 连接复用 (SMux)"),
		);
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
						node_id: fixtureCatalog.slotString.s118(),
						node_name: fixtureCatalog.slotString.s33(),
						access_host: fixtureCatalog.slotString.s99(),
						api_base_url: fixtureCatalog.slotString.s123(),
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
						endpoint_id: fixtureCatalog.slotString.s120(),
						node_id: fixtureCatalog.slotString.s118(),
						tag: fixtureCatalog.slotString.s121(),
						kind: "vless_reality_vision_tcp",
						port: 53844,
						meta: {
							reality: {
								dest: fixtureCatalog.slotString.s2(),
								server_names: fixtureCatalog.slotList.l5(),
								server_names_source: "manual",
								fingerprint: "chrome",
							},
							managed_default: true,
							canary_upstream: fixtureCatalog.slotString.s122(),
							accepted_authorities: fixtureCatalog.slotList.l6(),
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
						node_id: fixtureCatalog.slotString.s124(),
						node_name: fixtureCatalog.slotString.s125(),
						access_host: fixtureCatalog.slotString.s126(),
						api_base_url: fixtureCatalog.slotString.s127(),
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
						endpoint_id: fixtureCatalog.slotString.s128(),
						node_id: fixtureCatalog.slotString.s124(),
						tag: fixtureCatalog.slotString.s129(),
						kind: "vless_reality_vision_tcp",
						port: 53844,
						meta: {
							reality: {
								dest: fixtureCatalog.slotString.s2(),
								server_names: fixtureCatalog.slotList.l7(),
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
