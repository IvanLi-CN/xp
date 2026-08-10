import type { Meta, StoryObj } from "@storybook/react";
import { expect, screen, userEvent, within } from "@storybook/test";

import type { AdminEndpoint } from "../api/adminEndpoints";
import type { AdminNode } from "../api/adminNodes";
import { fixtureCatalog } from "../fixture-policy/catalog";
import { buildDenseNodeIpUsageStories } from "../storybook/ipUsageStoryData";
import { buildDenseNodeTcpConnectionStories } from "../storybook/tcpConnectionStoryData";

const node: AdminNode = {
	node_id: fixtureCatalog.identifier.nodePrimary(),
	node_name: fixtureCatalog.identifier.nodeNamePrimary(),
	access_host: fixtureCatalog.host.primary(),
	api_base_url: fixtureCatalog.url.primaryApi(),
	quota_limit_bytes: 0,
	quota_reset: {
		policy: "monthly",
		day_of_month: 1,
		tz_offset_minutes: null,
	},
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
		port: 8388,
		meta: {},
	},
	{
		endpoint_id: fixtureCatalog.identifier.endpointSecondary(),
		node_id: fixtureCatalog.identifier.nodePrimary(),
		tag: fixtureCatalog.identifier.endpointTagSecondary(),
		kind: fixtureCatalog.endpoint.vlessKind(),
		port: 443,
		meta: {
			dest: fixtureCatalog.address.loopback49043(),
			server_names: fixtureCatalog.list.primaryServerNames(),
			server_names_source: "manual",
			fingerprint: "chrome",
		},
	},
];

const ipUsageReports = buildDenseNodeIpUsageStories();
const tcpConnectionReports = buildDenseNodeTcpConnectionStories();

const meta = {
	title: "Pages/NodeDetailsPage",
	render: () => <div />,
	parameters: {
		router: {
			initialEntry: `/nodes/${node.node_id}`,
		},
		mockApi: {
			data: {
				nodes: [node],
				nodeIpUsageByNodeId: {
					[node.node_id]: ipUsageReports,
				},
				nodeTcpConnectionsByNodeId: {
					[node.node_id]: tcpConnectionReports,
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
			await canvas.findByRole("heading", { name: /service runtime/i }),
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
			await canvas.findByText("TCP connection count"),
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
			await canvas.findByRole("heading", { name: "Traffic" }),
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

export const DeleteWithEndpointCleanup: Story = {
	parameters: {
		mockApi: {
			data: {
				nodes: [node],
				endpoints: nodeEndpoints,
				nodeIpUsageByNodeId: {
					[node.node_id]: ipUsageReports,
				},
				nodeTcpConnectionsByNodeId: {
					[node.node_id]: tcpConnectionReports,
				},
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(
			await canvas.findByRole("tab", { name: "Danger zone" }),
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

export const RuntimeHistoryFallback: Story = {
	parameters: {
		router: {
			initialEntry: `/nodes/${node.node_id}`,
		},
		mockApi: {
			data: {
				nodes: [node],
				nodeIpUsageByNodeId: {
					[node.node_id]: ipUsageReports,
				},
				nodeTcpConnectionsByNodeId: {
					[node.node_id]: tcpConnectionReports,
				},
				nodeHistoryByNodeId: {
					[node.node_id]: {
						node_id: fixtureCatalog.identifier.nodePrimary(),
						last_synced_at: fixtureCatalog.timestamp.baseline(),
						last_sync_error: "request timeout while syncing node history",
						daily_traffic: [
							{
								date: "2026-05-20",
								uplink_bytes: fixtureCatalog.slotNumber.n2(),
								downlink_bytes: fixtureCatalog.slotNumber.n3(),
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
										observed_at: fixtureCatalog.slotString.s653(),
									},
									{
										component: "xray",
										status: "down",
										observed_at: fixtureCatalog.slotString.s653(),
									},
									{
										component: "cloudflared",
										status: "up",
										observed_at: fixtureCatalog.slotString.s653(),
									},
								],
							},
						],
						component_status_events: [
							{
								event_id: "fallback-event-1",
								occurred_at: fixtureCatalog.slotString.s654(),
								component: "xray",
								message: "xray status changed: up -> down",
								from_status: "up",
								to_status: "down",
							},
						],
					},
				},
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
