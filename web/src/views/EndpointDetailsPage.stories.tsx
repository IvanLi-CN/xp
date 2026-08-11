import type { Meta, StoryObj } from "@storybook/react";
import { expect, fireEvent, userEvent, within } from "@storybook/test";
import { fixtureCatalog } from "../fixture-policy/catalog";

const ENDPOINT_ID = fixtureCatalog.endpointId.fixture120();

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
						node_id: fixtureCatalog.nodeId.fixture118(),
						node_name: fixtureCatalog.nodeName.fixture33(),
						access_host: fixtureCatalog.host.fixture119(),
						api_base_url: fixtureCatalog.service.fixture34(),
						quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
						quota_reset: fixtureCatalog.quota.resetNode(),
					},
				],
				endpoints: [
					{
						endpoint_id: fixtureCatalog.endpointId.fixture120(),
						node_id: fixtureCatalog.nodeId.fixture118(),
						tag: fixtureCatalog.endpointTag.fixture121(),
						kind: fixtureCatalog.endpoint.vlessKind(),
						port: fixtureCatalog.endpoint.port53844(),
						meta: {
							reality: {
								dest: fixtureCatalog.address.loopbackPort39002(),
								server_names: fixtureCatalog.hostList.edge5(),
								server_names_source: "manual",
								fingerprint: "chrome",
							},
							managed_default: true,
							transport: "xhttp",
							canary_upstream: fixtureCatalog.canaryUpstream.httpLoopback(),
							accepted_authorities: fixtureCatalog.hostList.edge6(),
						},
						short_ids: fixtureCatalog.endpoint.shortIds(),
						active_short_id: fixtureCatalog.endpoint.activeShortId(),
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
			await canvas.findAllByText(fixtureCatalog.hostList.edge6()[0]),
		).toHaveLength(2);
		await expect(
			await canvas.findAllByText(fixtureCatalog.hostList.edge6()[1]),
		).toHaveLength(2);
		await expect(
			await canvas.findByText(
				"Accept additional ordinary HTTPS Host headers for camouflage routing. Omit port to use HTTPS default 443. This does not change REALITY serverNames or the canonical /generate_204 URL.",
			),
		).toBeInTheDocument();
	},
};

export const VlessXhttpTransport: Story = {
	tags: ["coverage-ui", "endpoint-vless-xhttp"],
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(await canvas.findByText("Advanced: VLESS transport"));
		await expect(
			await canvas.findByRole("radio", { name: "XHTTP / XMUX" }),
		).toBeChecked();
		await expect(
			await canvas.findByText(
				"Recommended. Mihomo YAML uses one reusable HTTP/2 connection after pool warm-up.",
			),
		).toBeInTheDocument();
		await expect(
			await canvas.findByText(/Raw URI includes Mihomo-specific XMUX settings/),
		).toBeInTheDocument();
		await expect(
			await canvas.findByText(/Changing this mode rebuilds the inbound/),
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
						endpoint_id: fixtureCatalog.endpointId.fixture120(),
						node_id: fixtureCatalog.nodeId.fixture118(),
						tag: fixtureCatalog.endpointTag.fixture121(),
						kind: fixtureCatalog.endpoint.ssKind(),
						port: fixtureCatalog.endpoint.port443(),
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
						node_id: fixtureCatalog.nodeId.fixture118(),
						node_name: fixtureCatalog.nodeName.fixture33(),
						access_host: fixtureCatalog.host.fixture99(),
						api_base_url: fixtureCatalog.service.fixture123(),
						quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
						quota_reset: fixtureCatalog.quota.resetNode(),
					},
				],
				endpoints: [
					{
						endpoint_id: fixtureCatalog.endpointId.fixture120(),
						node_id: fixtureCatalog.nodeId.fixture118(),
						tag: fixtureCatalog.endpointTag.fixture121(),
						kind: fixtureCatalog.endpoint.vlessKind(),
						port: fixtureCatalog.endpoint.port53844(),
						meta: {
							reality: {
								dest: fixtureCatalog.address.loopbackPort39002(),
								server_names: fixtureCatalog.hostList.edge5(),
								server_names_source: "manual",
								fingerprint: "chrome",
							},
							managed_default: true,
							canary_upstream: fixtureCatalog.canaryUpstream.httpLoopback(),
							accepted_authorities: fixtureCatalog.hostList.edge6(),
						},
						short_ids: fixtureCatalog.endpoint.shortIds(),
						active_short_id: fixtureCatalog.endpoint.activeShortId(),
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
		fireEvent.change(input, {
			target: { value: fixtureCatalog.host.primary() },
		});
		await userEvent.click(
			await within(tagInputControl).findByRole("button", { name: "Add" }),
		);
		await expect(
			await within(tagInputControl).findByTitle(
				`${fixtureCatalog.host.primary()}:443`,
			),
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
			).findByText(`https://${fixtureCatalog.address.loopbackPort39002()}`),
		);
		await expect(
			await canvas.findByLabelText("canary upstream url"),
		).toHaveValue(`https://${fixtureCatalog.address.loopbackPort39002()}`);

		await userEvent.click(
			await canvas.findByRole("button", {
				name: "Show access host suggestions",
			}),
		);
		await userEvent.click(
			await within(
				await within(document.body).findByTestId("tag-input-suggestions"),
			).findByText(fixtureCatalog.authority.host119Port53844()[0]),
		);
		await expect(
			await canvas.findByTitle(fixtureCatalog.authority.host119Port53844()[0]),
		).toBeInTheDocument();
	},
};

export const ManagedDefaultNodeAliasSuggestionsWithoutUpstreamHistory: Story = {
	tags: ["managed-vless-autocomplete"],
	parameters: {
		router: {
			initialEntry: `/endpoints/${fixtureCatalog.endpointId.fixture128()}`,
		},
		mockApi: {
			data: {
				nodes: [
					{
						node_id: fixtureCatalog.nodeId.fixture124(),
						node_name: fixtureCatalog.nodeName.fixture125(),
						access_host: fixtureCatalog.host.fixture126(),
						api_base_url: fixtureCatalog.service.fixture127(),
						quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
						quota_reset: fixtureCatalog.quota.resetNode(),
					},
				],
				endpoints: [
					{
						endpoint_id: fixtureCatalog.endpointId.fixture128(),
						node_id: fixtureCatalog.nodeId.fixture124(),
						tag: fixtureCatalog.endpointTag.fixture129(),
						kind: fixtureCatalog.endpoint.vlessKind(),
						port: fixtureCatalog.endpoint.port53844(),
						meta: {
							reality: {
								dest: fixtureCatalog.address.loopbackPort39002(),
								server_names: fixtureCatalog.hostList.edge7(),
								server_names_source: "manual",
								fingerprint: "chrome",
							},
							managed_default: true,
						},
						short_ids: fixtureCatalog.endpoint.shortIds(),
						active_short_id: fixtureCatalog.endpoint.activeShortId(),
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
			).findByText(`https://${fixtureCatalog.address.loopbackPort39002()}`),
		);
		await expect(
			await canvas.findByLabelText("canary upstream url"),
		).toHaveValue(`https://${fixtureCatalog.address.loopbackPort39002()}`);

		await userEvent.click(
			await canvas.findByRole("button", {
				name: "Show access host suggestions",
			}),
		);
		await userEvent.click(
			await within(
				await within(document.body).findByTestId("tag-input-suggestions"),
			).findByText(fixtureCatalog.authority.host126Port53844()[0]),
		);
		await expect(
			await canvas.findByTitle(fixtureCatalog.authority.host126Port53844()[0]),
		).toBeInTheDocument();
	},
};
