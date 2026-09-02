import type { Meta, StoryObj } from "@storybook/react";
import { expect, screen, userEvent, within } from "@storybook/test";

import type { AdminEndpoint } from "../api/adminEndpoints";
import type {
	AdminMembershipOperation,
	AdminNode,
	AdminNodeMihomoResourcePolicy,
} from "../api/adminNodes";
import { fixtureCatalog } from "../fixture-policy/catalog";
import { buildDenseNodeIpUsageStories } from "../storybook/ipUsageStoryData";
import { buildDenseNodeTcpConnectionStories } from "../storybook/tcpConnectionStoryData";

const node: AdminNode = {
	node_id: fixtureCatalog.identifier.nodePrimary(),
	node_name: fixtureCatalog.identifier.nodeNamePrimary(),
	access_host: fixtureCatalog.host.primary(),
	api_base_url: fixtureCatalog.url.primaryApi(),
	quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
	quota_reset: fixtureCatalog.quota.resetNode(),
	egress_probe: {
		public_ipv4: fixtureCatalog.address.tertiaryIpv4(),
		public_ipv6: "2001:db8::8",
		selected_public_ip: fixtureCatalog.address.tertiaryIpv4(),
		country_code: "TW",
		geo_region: "Taiwan",
		geo_city: "Taipei",
		geo_operator: "ExampleNet",
		subscription_region: "taiwan",
		checked_at: fixtureCatalog.timestamp.recent(),
		last_success_at: fixtureCatalog.timestamp.recent(),
		stale: false,
		error_summary: null,
	},
};

const nodeEndpoints: AdminEndpoint[] = [
	{
		endpoint_id: fixtureCatalog.identifier.endpointPrimary(),
		node_id: fixtureCatalog.identifier.nodePrimary(),
		tag: fixtureCatalog.identifier.endpointTagPrimary(),
		kind: fixtureCatalog.endpoint.ssKind(),
		port: fixtureCatalog.endpoint.port8388(),
		meta: {},
	},
	{
		endpoint_id: fixtureCatalog.identifier.endpointSecondary(),
		node_id: fixtureCatalog.identifier.nodePrimary(),
		tag: fixtureCatalog.identifier.endpointTagSecondary(),
		kind: fixtureCatalog.endpoint.vlessKind(),
		port: fixtureCatalog.endpoint.port443(),
		meta: {
			dest: fixtureCatalog.address.loopback49043(),
			server_names: fixtureCatalog.list.primaryServerNames(),
			server_names_source: "manual",
			fingerprint: "chrome",
		},
	},
];

const pendingDeleteOperation: AdminMembershipOperation = {
	operation_id: "e9a42d3c-9812-4a38-8a23-2d8cb7770001",
	kind: "remove_node",
	raft_node_id: 42,
	node_id: fixtureCatalog.identifier.nodePrimary(),
	expected_membership: "storybook-membership-revision",
	phase: "prepared",
	delete_endpoints: false,
	expected_endpoint_ids: [],
	created_at: fixtureCatalog.timestamp.t20260308T005900(),
	next_retry_at: fixtureCatalog.timestamp.t20260308T000100(),
	terminal_at: null,
	evidence: "waiting for Raft membership removal",
};

const blockedDeleteOperation: AdminMembershipOperation = {
	...pendingDeleteOperation,
	operation_id: "43c32c41-c0f4-4240-b51d-f44e68f3f1f1",
	phase: "blocked",
	next_retry_at: null,
	terminal_at: fixtureCatalog.timestamp.t20260308T000200(),
	evidence: "membership revision changed while the operation was pending",
};

const ipUsageReports = buildDenseNodeIpUsageStories();
const tcpConnectionReports = buildDenseNodeTcpConnectionStories();

const meta = {
	title: "Pages/NodeDetailsPage",
	tags: ["autodocs"],
	render: () => <div />,
	parameters: {
		router: {
			initialEntry: `/nodes/${node.node_id}`,
		},
		mockApi: {
			data: {
				nodes: [node],
				nodeIpUsageByNodeId: Object.fromEntries([
					[node.node_id, ipUsageReports],
				]),
				nodeTcpConnectionsByNodeId: Object.fromEntries([
					[node.node_id, tcpConnectionReports],
				]),
				nodeMihomoResourcePolicyByNodeId: {
					[fixtureCatalog.identifier.nodePrimary()]: {
						node_id: fixtureCatalog.identifier.nodePrimary(),
						deployment_default_cidrs: [fixtureCatalog.address.privateCidr()],
						override_cidrs: [fixtureCatalog.address.privateCidr()],
						effective_cidrs: [fixtureCatalog.address.privateCidr()],
						source: "override",
						status: "healthy",
						error: null,
					} satisfies AdminNodeMihomoResourcePolicy,
				},
			},
		},
	},
} satisfies Meta;

export default meta;

type Story = StoryObj<typeof meta>;

export const Default: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			await canvas.findByRole("tab", { name: "Service runtime" }),
		).toBeInTheDocument();
		await expect(await canvas.findByText(/^key events$/i)).toBeInTheDocument();
	},
};

export const IpUsageTab: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(await canvas.findByRole("tab", { name: "IP usage" }));
		await expect(
			await canvas.findByRole("button", {
				name: fixtureCatalog.address.secondaryIpv4(),
			}),
		).toBeInTheDocument();
	},
};

export const IpUsageTab7d: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(await canvas.findByRole("tab", { name: "IP usage" }));
		await userEvent.click(await canvas.findByRole("button", { name: "7d" }));
		await expect(
			await canvas.findByRole("button", { name: "7d" }),
		).toHaveAttribute("aria-pressed", "true");
		await expect(
			await canvas.findByRole("button", {
				name: fixtureCatalog.address.secondaryIpv4(),
			}),
		).toBeInTheDocument();
	},
};

export const TcpConnectionsTab: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(
			await canvas.findByRole("tab", { name: "TCP connections" }),
		);
		await expect(
			await canvas.findByText(
				/Per-minute ESTABLISHED inbound TCP connections/i,
			),
		).toBeInTheDocument();
		await expect(
			await canvas.findByText(/Connections per minute/i),
		).toBeInTheDocument();
		await expect(
			await canvas.findByText(
				new RegExp(`${fixtureCatalog.identifier.endpointTagPrimary()} :443`),
			),
		).toBeInTheDocument();
		await expect(
			await canvas.findByText(/Combined across selected endpoints/i),
		).toBeInTheDocument();
	},
};

export const TrafficTab: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(await canvas.findByRole("tab", { name: "Traffic" }));
		await expect(
			await canvas.findByRole("tab", { name: "Traffic" }),
		).toBeInTheDocument();
		await userEvent.click(
			await canvas.findByRole("button", { name: "Last 31 days" }),
		);
		await expect(
			await canvas.findByRole("button", { name: "Last 31 days" }),
		).toBeInTheDocument();
	},
};

export const MetadataEgressProbe: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(
			await canvas.findByRole("tab", { name: "Node metadata" }),
		);
		await expect(
			await canvas.findByText("Node egress probe"),
		).toBeInTheDocument();
		await expect(
			await canvas.findAllByText(fixtureCatalog.address.tertiaryIpv4()),
		).toHaveLength(2);
		await expect(await canvas.findByText("ExampleNet")).toBeInTheDocument();
	},
};

export const NodeSettings: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(
			await canvas.findByRole("tab", { name: "Node settings" }),
		);
		await expect(
			await canvas.findByRole("heading", { name: "Mihomo resources" }),
		).toBeInTheDocument();
		await expect(
			await canvas.findByRole("heading", { name: "Quota settings" }),
		).toBeInTheDocument();
		await expect(
			await canvas.findByRole("heading", { name: "Danger zone" }),
		).toBeInTheDocument();
	},
};

export const NodeSettingsMobile: Story = {
	parameters: {
		viewport: {
			defaultViewport: "nodeSettingsMobile",
			viewports: {
				nodeSettingsMobile: {
					name: "Node settings mobile",
					styles: { width: "393px", height: "852px" },
				},
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(
			await canvas.findByRole("tab", { name: "Node settings" }),
		);
		await expect(
			await canvas.findByRole("heading", { name: "Mihomo resources" }),
		).toBeInTheDocument();
		await expect(
			await canvas.findByRole("heading", { name: "Quota settings" }),
		).toBeInTheDocument();
		await expect(
			await canvas.findByRole("heading", { name: "Danger zone" }),
		).toBeInTheDocument();
	},
};

async function verifyMihomoPrivateResourcePolicy(canvasElement: HTMLElement) {
	const canvas = within(canvasElement);
	await canvas.findByText("Node settings", { exact: true });
	const settingsTab = Array.from(
		canvasElement.querySelectorAll<HTMLElement>('[role="tab"]'),
	).find((element) => element.textContent?.trim() === "Node settings");
	if (settingsTab) {
		settingsTab.click();
	} else {
		const mobileSelect = canvasElement.querySelector<HTMLElement>(
			'[role="combobox"][aria-label="Node details section"]',
		);
		if (!mobileSelect) throw new Error("Mihomo navigation control is missing");
		mobileSelect.click();
		await userEvent.click(
			await within(canvasElement.ownerDocument.body).findByRole("option", {
				name: "Node settings",
			}),
		);
	}
	await expect(
		await canvas.findByRole("heading", { name: "Mihomo resources" }),
	).toBeInTheDocument();
	await expect(
		await canvas.findByText("Mihomo private resource policy"),
	).toBeInTheDocument();
	await expect(
		await canvas.findByRole("textbox", { name: "Web override CIDRs" }),
	).toBeInTheDocument();
	await expect(
		await canvas.findByRole("button", { name: "Remove" }),
	).toBeInTheDocument();
	await expect(
		await canvas.findByRole("button", { name: "Save override" }),
	).toBeInTheDocument();
	await expect(
		await canvas.findByRole("button", { name: "Disable private targets" }),
	).toBeInTheDocument();
	await expect(
		await canvas.findByRole("button", { name: "Restore deployment default" }),
	).toBeInTheDocument();
}

export const MihomoPrivateResourcePolicyDesktop: Story = {
	play: async ({ canvasElement }) => {
		await verifyMihomoPrivateResourcePolicy(canvasElement);
	},
};

export const MihomoPrivateResourcePolicy: Story = {
	parameters: {
		viewport: {
			defaultViewport: "mihomoPolicyMobile",
			viewports: {
				mihomoPolicyMobile: {
					name: "Mihomo policy mobile",
					styles: { width: "393px", height: "852px" },
				},
			},
		},
	},
	play: async ({ canvasElement }) => {
		await verifyMihomoPrivateResourcePolicy(canvasElement);
	},
};

export const MihomoPrivateResourcePolicyIpv6Mobile: Story = {
	parameters: {
		viewport: {
			defaultViewport: "mihomoPolicyMobile",
			viewports: {
				mihomoPolicyMobile: {
					name: "Mihomo policy mobile",
					styles: { width: "393px", height: "852px" },
				},
			},
		},
		mockApi: {
			data: {
				nodeMihomoResourcePolicyByNodeId: {
					[fixtureCatalog.identifier.nodePrimary()]: {
						node_id: fixtureCatalog.identifier.nodePrimary(),
						deployment_default_cidrs: [
							fixtureCatalog.address.privateIpv6Cidr(),
						],
						override_cidrs: [fixtureCatalog.address.privateIpv6Cidr()],
						effective_cidrs: [fixtureCatalog.address.privateIpv6Cidr()],
						source: "override",
						status: "healthy",
						error: null,
					} satisfies AdminNodeMihomoResourcePolicy,
				},
			},
		},
	},
	play: async ({ canvasElement }) => {
		await verifyMihomoPrivateResourcePolicy(canvasElement);
		const canvas = within(canvasElement);
		await expect(
			await canvas.findAllByText(fixtureCatalog.address.privateIpv6Cidr()),
		).toHaveLength(3);
	},
};

export const DeleteWithEndpointCleanup: Story = {
	parameters: {
		mockApi: {
			data: {
				nodes: [node],
				endpoints: nodeEndpoints,
				nodeIpUsageByNodeId: Object.fromEntries([
					[node.node_id, ipUsageReports],
				]),
				nodeTcpConnectionsByNodeId: Object.fromEntries([
					[node.node_id, tcpConnectionReports],
				]),
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(
			await canvas.findByRole("tab", { name: "Node settings" }),
		);
		await userEvent.click(
			await canvas.findByRole("button", { name: "Delete node" }),
		);
		await expect(
			await screen.findByText("Endpoints to delete: 2"),
		).toBeVisible();
		await expect(
			await screen.findByText(fixtureCatalog.identifier.endpointTagPrimary()),
		).toBeVisible();
		await expect(
			await screen.findByText(fixtureCatalog.identifier.endpointTagSecondary()),
		).toBeVisible();
		await expect(
			await screen.findByRole("button", {
				name: "Delete node and endpoints",
			}),
		).toBeVisible();
	},
};

export const DeletePending: Story = {
	parameters: {
		mockApi: {
			data: {
				nodes: [node],
			},
			nodeDeleteAccepted: {
				nodeId: fixtureCatalog.identifier.nodePrimary(),
				operation: pendingDeleteOperation,
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(
			await canvas.findByRole("tab", { name: "Node settings" }),
		);
		await userEvent.click(
			await canvas.findByRole("button", { name: "Delete node" }),
		);
		await userEvent.click(
			await screen.findByRole("button", { name: "Delete" }),
		);
		await expect(await canvas.findByRole("status")).toHaveTextContent(
			"Node deletion is continuing.",
		);
		await expect(
			await canvas.findByRole("button", { name: "Delete node" }),
		).toBeDisabled();
	},
};

export const DeleteBlocked: Story = {
	parameters: {
		mockApi: {
			data: {
				nodes: [node],
			},
			nodeDeleteAccepted: {
				nodeId: fixtureCatalog.identifier.nodePrimary(),
				operation: blockedDeleteOperation,
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(
			await canvas.findByRole("tab", { name: "Node settings" }),
		);
		await userEvent.click(
			await canvas.findByRole("button", { name: "Delete node" }),
		);
		await userEvent.click(
			await screen.findByRole("button", { name: "Delete" }),
		);
		await expect(await canvas.findByRole("status")).toHaveTextContent(
			"Node deletion is blocked.",
		);
		await expect(
			await canvas.findByText(
				"membership revision changed while the operation was pending",
			),
		).toBeVisible();
	},
};

export const RuntimeHistoryFallback: Story = {
	parameters: {
		router: {
			initialEntry: `/nodes/${node.node_id}`,
		},
		mockApi: {
			data: {
				nodes: [node],
				nodeIpUsageByNodeId: Object.fromEntries([
					[node.node_id, ipUsageReports],
				]),
				nodeTcpConnectionsByNodeId: Object.fromEntries([
					[node.node_id, tcpConnectionReports],
				]),
				nodeHistoryByNodeId: Object.fromEntries([
					[
						node.node_id,
						{
							node_id: fixtureCatalog.identifier.nodePrimary(),
							last_synced_at: fixtureCatalog.timestamp.baseline(),
							last_sync_error: "request timeout while syncing node history",
							daily_traffic: [
								{
									date: "2026-05-20",
									uplink_bytes: fixtureCatalog.number.value3(),
									downlink_bytes: fixtureCatalog.number.value4(),
									updated_at: fixtureCatalog.timestamp.baseline(),
								},
							],
							daily_component_status: [
								{
									date: "2026-05-20",
									components: [
										{
											component: "xp",
											status: "up",
											observed_at: fixtureCatalog.timestamp.t20260520T080000(),
										},
										{
											component: "xray",
											status: "down",
											observed_at: fixtureCatalog.timestamp.t20260520T080000(),
										},
										{
											component: "cloudflared",
											status: "up",
											observed_at: fixtureCatalog.timestamp.t20260520T080000(),
										},
									],
								},
							],
							component_status_events: [
								{
									event_id: "fallback-event-1",
									occurred_at: fixtureCatalog.timestamp.t20260520T074200(),
									component: "xray",
									message: "xray status changed: up -> down",
									from_status: "up",
									to_status: "down",
								},
							],
						},
					],
				]),
			},
			failNodeRuntimeNodeIds: [node.node_id],
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			await canvas.findByText(/Live node API is unavailable/i),
		).toBeInTheDocument();
		await expect(
			await canvas.findByText("Daily traffic mirror"),
		).toBeInTheDocument();
		await expect(
			await canvas.findByText("Daily component snapshot"),
		).toBeInTheDocument();
	},
};

export const OfflineCachedRuntime: Story = {
	parameters: {
		appRuntime: {
			initialIsOnline: false,
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			await canvas.findByText(/offline cached/i),
		).toBeInTheDocument();
		await expect(
			await canvas.findByText(/offline node snapshot/i),
		).toBeInTheDocument();
		await expect(
			await canvas.findByText(/last successful sync:/i),
		).toBeInTheDocument();
	},
};
