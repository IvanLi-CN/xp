import type { Meta, StoryObj } from "@storybook/react";
import { expect, fireEvent, userEvent, within } from "@storybook/test";
import { fixtureCatalog } from "../fixture-policy/catalog";

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
						node_id: fixtureCatalog.nodeId.fixture118(),
						node_name: fixtureCatalog.nodeName.fixture86(),
						access_host: fixtureCatalog.host.fixture130(),
						api_base_url: fixtureCatalog.service.fixture131(),
						quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
						quota_reset: fixtureCatalog.quota.resetNode(),
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
		await userEvent.click(await canvas.findByText("Advanced: VLESS transport"));
		await expect(
			await canvas.findByRole("radio", { name: "XHTTP / XMUX" }),
		).toBeChecked();
		await expect(
			await canvas.findByText(
				"Recommended. Mihomo YAML uses one reusable HTTP/2 connection after pool warm-up.",
			),
		).toBeInTheDocument();
		await expect(canvas.queryByLabelText("dest")).toBeNull();
		await expect(canvas.queryByLabelText("serverNames")).toBeNull();
	},
};

export const MihomoSmuxDefaults: Story = {
	tags: ["coverage-ui", "endpoint-mihomo-smux"],
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(await canvas.findByLabelText("Kind"));
		await userEvent.click(
			await within(document.body).findByRole("option", {
				name: "SS2022 BLAKE3 AES-128-GCM",
			}),
		);
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

export const ManagedDefaultAutocompleteSuggestions: Story = {
	tags: ["coverage-ui", "managed-vless-autocomplete"],
	parameters: {
		mockApi: {
			data: {
				nodes: [
					{
						node_id: fixtureCatalog.nodeId.fixture118(),
						node_name: fixtureCatalog.nodeName.fixture86(),
						access_host: fixtureCatalog.host.fixture130(),
						api_base_url: fixtureCatalog.service.fixture131(),
						quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
						quota_reset: fixtureCatalog.quota.resetNode(),
					},
				],
				endpoints: [
					{
						endpoint_id: fixtureCatalog.endpointId.fixture132(),
						node_id: fixtureCatalog.nodeId.fixture118(),
						tag: fixtureCatalog.endpointTag.fixture133(),
						kind: fixtureCatalog.endpoint.vlessKind(),
						port: fixtureCatalog.endpoint.port443(),
						meta: {
							reality: {
								dest: fixtureCatalog.address.loopbackPort39111(),
								server_names: fixtureCatalog.hostList.edge8(),
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
		).toEqual([
			`https://${fixtureCatalog.address.loopbackPort39111()}`,
			fixtureCatalog.canaryUpstream.httpsListener().url,
		]);
		await userEvent.click(
			await within(suggestionPanel).findByText(
				`https://${fixtureCatalog.address.loopbackPort39111()}`,
			),
		);
		await expect(await canvas.findByLabelText("canaryUpstreamUrl")).toHaveValue(
			`https://${fixtureCatalog.address.loopbackPort39111()}`,
		);

		await userEvent.click(
			await canvas.findByRole("button", {
				name: "Show access host suggestions",
			}),
		);
		await userEvent.click(
			await within(
				await within(document.body).findByTestId("tag-input-suggestions"),
			).findByText(fixtureCatalog.authority.host130Port8443()[0]),
		);
		await expect(
			await canvas.findByTitle(fixtureCatalog.authority.host130Port8443()[0]),
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
						node_id: fixtureCatalog.nodeId.fixture124(),
						node_name: fixtureCatalog.nodeName.fixture125(),
						access_host: fixtureCatalog.host.fixture126(),
						api_base_url: fixtureCatalog.service.fixture127(),
						quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
						quota_reset: fixtureCatalog.quota.resetNode(),
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
			).findByText(fixtureCatalog.canaryUpstream.httpsListener().url),
		);
		await expect(await canvas.findByLabelText("canaryUpstreamUrl")).toHaveValue(
			fixtureCatalog.canaryUpstream.httpsListener().url,
		);

		await userEvent.click(
			await canvas.findByRole("button", {
				name: "Show access host suggestions",
			}),
		);
		await userEvent.click(
			await within(
				await within(document.body).findByTestId("tag-input-suggestions"),
			).findByText(fixtureCatalog.host.fixture126()),
		);
		await expect(
			await canvas.findByTitle(fixtureCatalog.authority.host126Port443()[0]),
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
						node_id: fixtureCatalog.nodeId.fixture118(),
						node_name: fixtureCatalog.nodeName.fixture86(),
						access_host: fixtureCatalog.host.fixture99(),
						api_base_url: fixtureCatalog.service.fixture123(),
						quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
						quota_reset: fixtureCatalog.quota.resetNode(),
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
