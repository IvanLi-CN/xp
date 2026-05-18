import type { Meta, StoryObj } from "@storybook/react";
import { expect, screen, userEvent, within } from "@storybook/test";

import type { AdminEndpoint } from "../api/adminEndpoints";
import type { AdminNode } from "../api/adminNodes";
import { buildDenseNodeIpUsageStories } from "../storybook/ipUsageStoryData";

const node: AdminNode = {
	node_id: "01J000000000000000000000001",
	node_name: "node-a",
	access_host: "node-a.example.invalid",
	api_base_url: "https://node-a.example.invalid",
	quota_limit_bytes: 0,
	quota_reset: {
		policy: "monthly",
		day_of_month: 1,
		tz_offset_minutes: null,
	},
	egress_probe: {
		public_ipv4: "203.0.113.8",
		public_ipv6: "2001:db8::8",
		selected_public_ip: "203.0.113.8",
		country_code: "TW",
		geo_region: "Taiwan",
		geo_city: "Taipei",
		geo_operator: "HiNet",
		subscription_region: "taiwan",
		checked_at: "2026-03-08T00:59:00Z",
		last_success_at: "2026-03-08T00:59:00Z",
		stale: false,
		error_summary: null,
	},
};

const nodeEndpoints: AdminEndpoint[] = [
	{
		endpoint_id: "01J00000000000000000000E01",
		node_id: node.node_id,
		tag: "node-a-ss",
		kind: "ss2022_2022_blake3_aes_128_gcm",
		port: 8388,
		meta: {},
	},
	{
		endpoint_id: "01J00000000000000000000E02",
		node_id: node.node_id,
		tag: "node-a-reality",
		kind: "vless_reality_vision_tcp",
		port: 443,
		meta: {
			dest: "www.cloudflare.com:443",
			server_names: ["node-a.example.invalid"],
			server_names_source: "manual",
			fingerprint: "chrome",
		},
	},
];

const ipUsageReports = buildDenseNodeIpUsageStories(node);

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
			await canvas.findByRole("button", { name: "198.51.100.88" }),
		).toBeInTheDocument();
		await expect(
			await canvas.findByRole("button", { name: "203.0.113.55" }),
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
			await canvas.findByRole("button", { name: "198.51.100.88" }),
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
		await expect(await canvas.findAllByText("203.0.113.8")).toHaveLength(2);
		await expect(await canvas.findByText("HiNet")).toBeInTheDocument();
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
		await expect(await screen.findByText("node-a-ss")).toBeVisible();
		await expect(await screen.findByText("node-a-reality")).toBeVisible();
		await expect(
			await screen.findByRole("button", {
				name: "Delete node and endpoints",
			}),
		).toBeVisible();
	},
};
