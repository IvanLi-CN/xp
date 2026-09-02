import type { Meta, StoryObj } from "@storybook/react";
import { expect, screen, userEvent, within } from "@storybook/test";

import { DEFAULT_API_CAPABILITIES } from "../../.storybook/mocks/apiMockContract";
import type { ResourceMonitoringMockFixtures } from "../../.storybook/mocks/resourceMonitoringMock";
import type { AdminEndpoint } from "../api/adminEndpoints";
import type {
	AdminMembershipOperation,
	AdminNode,
	AdminNodeMihomoResourcePolicy,
} from "../api/adminNodes";
import type {
	NodeResourceHistoryMetric,
	ResourceHistoryResponse,
	ResourceRole,
	ResourceSnapshot,
	RuntimeResourceHistoryMetric,
} from "../api/adminResources";
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

const supportedMeasurement = (value: number) => ({
	capability: "supported" as const,
	value,
});

const unsupportedMeasurement = (reason_code: string) => ({
	capability: "unsupported" as const,
	reason_code,
});

function resourceObservedAt(index: number): string {
	const minute = String(Math.floor(index / 4)).padStart(2, "0");
	const second = String((index % 4) * 15).padStart(2, "0");
	return `2026-09-01T10:${minute}:${second}Z`;
}

const resourceSnapshot: ResourceSnapshot = {
	node_id: node.node_id,
	observed_at: "2026-09-01T10:05:45Z",
	resource_domain: "host",
	capture_state: "active",
	capability: "supported",
	domain: {
		cpu_busy_percent: supportedMeasurement(48.2),
		cpu_iowait_percent: supportedMeasurement(1.6),
		load1: supportedMeasurement(1.4),
		memory_total_bytes: supportedMeasurement(16 * 1024 ** 3),
		memory_available_bytes: supportedMeasurement(7.3 * 1024 ** 3),
		swap_total_bytes: supportedMeasurement(2 * 1024 ** 3),
		swap_free_bytes: supportedMeasurement(2 * 1024 ** 3),
		filesystems: [
			{
				mount: "/",
				capability: "supported",
				total_bytes: 100 * 1024 ** 3,
				available_bytes: 64 * 1024 ** 3,
				used_percent: 36,
				total_inodes: 6_000_000,
				available_inodes: 5_200_000,
				used_inode_percent: 13.3,
			},
		],
	},
	runtimes: [
		{
			role: "xp",
			state: "managed",
			capability: "supported",
			metrics: {
				cpu_percent: supportedMeasurement(8.1),
				rss_bytes: supportedMeasurement(48 * 1024 * 1024),
				pss_bytes: supportedMeasurement(42 * 1024 * 1024),
				read_bytes_per_second: supportedMeasurement(12 * 1024),
				write_bytes_per_second: supportedMeasurement(8 * 1024),
				fd_count: supportedMeasurement(32),
				thread_count: supportedMeasurement(12),
			},
		},
		{
			role: "xray",
			state: "managed",
			capability: "supported",
			metrics: {
				cpu_percent: supportedMeasurement(18.4),
				rss_bytes: supportedMeasurement(96 * 1024 * 1024),
				pss_bytes: supportedMeasurement(83 * 1024 * 1024),
				read_bytes_per_second: supportedMeasurement(240 * 1024),
				write_bytes_per_second: supportedMeasurement(76 * 1024),
				fd_count: supportedMeasurement(188),
				thread_count: supportedMeasurement(21),
			},
		},
		{
			role: "cloudflared",
			state: "managed",
			capability: "supported",
			metrics: {
				cpu_percent: supportedMeasurement(3.2),
				rss_bytes: supportedMeasurement(28 * 1024 * 1024),
				pss_bytes: supportedMeasurement(24 * 1024 * 1024),
				read_bytes_per_second: supportedMeasurement(96 * 1024),
				write_bytes_per_second: supportedMeasurement(31 * 1024),
				fd_count: supportedMeasurement(48),
				thread_count: supportedMeasurement(9),
			},
		},
		{
			role: "canary",
			state: "managed",
			capability: "unsupported",
			metrics: {
				cpu_percent: unsupportedMeasurement("runtime_not_separable"),
				rss_bytes: unsupportedMeasurement("runtime_not_separable"),
				pss_bytes: unsupportedMeasurement("runtime_not_separable"),
				read_bytes_per_second: unsupportedMeasurement("runtime_not_separable"),
				write_bytes_per_second: unsupportedMeasurement("runtime_not_separable"),
				fd_count: unsupportedMeasurement("runtime_not_separable"),
				thread_count: unsupportedMeasurement("runtime_not_separable"),
			},
		},
	],
};

function resourceHistory(
	metric: NodeResourceHistoryMetric,
	valueAt: (index: number) => number,
): ResourceHistoryResponse {
	return {
		metric,
		role: null,
		resolution: "15s",
		quality: "complete",
		coverage: [1_788_256_800, 1_788_257_145],
		watermark: 1_788_257_145,
		gaps: [],
		freshness_seconds: 5,
		truncated: false,
		points: Array.from({ length: 24 }, (_, index) => ({
			observed_at: resourceObservedAt(index),
			value: valueAt(index),
			capability: "supported",
		})),
	};
}

function runtimeResourceHistory(
	role: ResourceRole,
	metric: string,
	valueAt: (index: number) => number,
): ResourceHistoryResponse {
	return {
		...resourceHistory("cpu_busy_percent", valueAt),
		metric,
		role,
	};
}

const resourceHistoryByMetric = {
	cpu_busy_percent: resourceHistory(
		"cpu_busy_percent",
		(index) => 30 + ((index * 7) % 35),
	),
	memory_available_bytes: resourceHistory(
		"memory_available_bytes",
		(index) => (7 + ((index * 3) % 5) / 10) * 1024 ** 3,
	),
	"filesystem.root.used_percent": resourceHistory(
		"filesystem.root.used_percent",
		(index) => 34 + ((index * 5) % 9),
	),
	cpu_iowait_percent: resourceHistory(
		"cpu_iowait_percent",
		(index) => 0.8 + ((index * 4) % 10) / 10,
	),
} satisfies Record<NodeResourceHistoryMetric, ResourceHistoryResponse>;

const RUNTIME_RESOURCE_HISTORY_METRICS = [
	"cpu_percent",
	"rss_bytes",
	"pss_bytes",
	"read_bytes_per_second",
	"write_bytes_per_second",
	"fd_count",
	"thread_count",
] as const satisfies readonly RuntimeResourceHistoryMetric[];

const runtimeHistoryBase = {
	xp: {
		cpu: 8,
		fd: 32,
		pss_mib: 42,
		read_kib_per_second: 12,
		rss_mib: 48,
		thread: 12,
		write_kib_per_second: 8,
	},
	xray: {
		cpu: 18,
		fd: 188,
		pss_mib: 83,
		read_kib_per_second: 240,
		rss_mib: 96,
		thread: 21,
		write_kib_per_second: 76,
	},
	cloudflared: {
		cpu: 3,
		fd: 48,
		pss_mib: 24,
		read_kib_per_second: 96,
		rss_mib: 28,
		thread: 9,
		write_kib_per_second: 31,
	},
} as const;

function runtimeHistoryValue(
	role: keyof typeof runtimeHistoryBase,
	metric: RuntimeResourceHistoryMetric,
	point: number,
): number {
	const base = runtimeHistoryBase[role];
	switch (metric) {
		case "cpu_percent":
			return base.cpu + point * 0.9;
		case "rss_bytes":
			return (base.rss_mib + point * 0.6) * 1024 ** 2;
		case "pss_bytes":
			return (base.pss_mib + point * 0.4) * 1024 ** 2;
		case "read_bytes_per_second":
			return (base.read_kib_per_second + point * 7) * 1024;
		case "write_bytes_per_second":
			return (base.write_kib_per_second + point * 3) * 1024;
		case "fd_count":
			return base.fd + point;
		case "thread_count":
			return base.thread + Math.floor(point / 2);
	}
}

function unsupportedRuntimeResourceHistory(
	role: ResourceRole,
	metric: RuntimeResourceHistoryMetric,
): ResourceHistoryResponse {
	return {
		...runtimeResourceHistory(role, metric, () => 0),
		points: Array.from({ length: 24 }, (_, index) => ({
			observed_at: resourceObservedAt(index),
			value: null,
			capability: "unsupported" as const,
		})),
	};
}

const runtimeHistoryByRole = {
	xp: Object.fromEntries(
		RUNTIME_RESOURCE_HISTORY_METRICS.map((metric) => [
			metric,
			runtimeResourceHistory("xp", metric, (point) =>
				runtimeHistoryValue("xp", metric, point),
			),
		]),
	),
	xray: Object.fromEntries(
		RUNTIME_RESOURCE_HISTORY_METRICS.map((metric) => [
			metric,
			runtimeResourceHistory("xray", metric, (point) =>
				runtimeHistoryValue("xray", metric, point),
			),
		]),
	),
	cloudflared: Object.fromEntries(
		RUNTIME_RESOURCE_HISTORY_METRICS.map((metric) => [
			metric,
			runtimeResourceHistory("cloudflared", metric, (point) =>
				runtimeHistoryValue("cloudflared", metric, point),
			),
		]),
	),
	canary: Object.fromEntries(
		RUNTIME_RESOURCE_HISTORY_METRICS.map((metric) => [
			metric,
			unsupportedRuntimeResourceHistory("canary", metric),
		]),
	),
} satisfies Partial<
	Record<ResourceRole, Record<string, ResourceHistoryResponse>>
>;

const resourceMonitoringFixtures = {
	snapshot: resourceSnapshot,
	historyByMetric: resourceHistoryByMetric,
	runtimeHistoryByRole,
} satisfies ResourceMonitoringMockFixtures;

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
				capabilities: {
					...DEFAULT_API_CAPABILITIES,
					capabilities: [
						...DEFAULT_API_CAPABILITIES.capabilities,
						"admin.resource-monitoring",
					],
				},
				nodes: [node],
				nodeIpUsageByNodeId: Object.fromEntries([
					[node.node_id, ipUsageReports],
				]),
				nodeTcpConnectionsByNodeId: Object.fromEntries([
					[node.node_id, tcpConnectionReports],
				]),
				nodeResourcesByNodeId: {
					[node.node_id]: resourceMonitoringFixtures.snapshot,
				},
				nodeResourceHistoryByNodeId: {
					[node.node_id]: resourceMonitoringFixtures.historyByMetric,
				},
				nodeResourceRuntimeHistoryByNodeId: {
					[node.node_id]: resourceMonitoringFixtures.runtimeHistoryByRole,
				},
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

export const ResourcesTab: Story = {
	tags: ["resource-monitoring"],
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(
			await canvas.findByRole("tab", { name: "Resources" }),
		);
		await expect(
			await canvas.findByText("Root filesystem use history"),
		).toBeInTheDocument();
		for (const label of [
			"CPU busy history chart",
			"Available memory history chart",
			"Root filesystem use history chart",
			"I/O wait history chart",
		]) {
			await expect(await canvas.findByLabelText(label)).toBeInTheDocument();
		}
	},
};

export const ResourcesRuntimeDetail: Story = {
	tags: ["resource-monitoring"],
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(
			await canvas.findByRole("tab", { name: "Resources" }),
		);
		for (const role of ["xp", "xray", "cloudflared", "canary"]) {
			await expect(
				await canvas.findByRole("button", {
					name: `Open ${role} resource details`,
				}),
			).toBeInTheDocument();
			await userEvent.click(
				await canvas.findByRole("button", {
					name: `Open ${role} resource details`,
				}),
			);
			await expect(
				await canvas.findByText(`${role} resource history`),
			).toBeInTheDocument();
		}
		await userEvent.click(
			await canvas.findByRole("button", {
				name: "Open xray resource details",
			}),
		);
		await expect(
			await canvas.findByText("xray resource history"),
		).toBeInTheDocument();
		for (const label of [
			"Runtime CPU history chart",
			"Runtime memory history chart",
			"Runtime throughput history chart",
			"Runtime file descriptor history chart",
			"Runtime thread history chart",
		]) {
			await expect(await canvas.findByLabelText(label)).toBeInTheDocument();
		}
		for (const label of [
			"Memory (RSS / PSS) legend",
			"Throughput (Read / Write) legend",
		]) {
			await expect(await canvas.findByLabelText(label)).toBeInTheDocument();
		}
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

export const MihomoPrivateResourcePolicyDraftDesktop: Story = {
	play: async ({ canvasElement }) => {
		await verifyMihomoPrivateResourcePolicy(canvasElement);
		const canvas = within(canvasElement);
		const input = await canvas.findByRole("textbox", {
			name: "Web override CIDRs",
		});
		await userEvent.type(input, "203.0.113.30");
		await userEvent.keyboard("{Enter}");
		await expect(input).toHaveValue("203.0.113.30");
		await expect(await canvas.findByRole("alert")).toHaveTextContent(
			"Only RFC1918 IPv4 CIDRs or IPv6 ULA CIDRs",
		);
		await expect(
			await canvas.findByRole("button", { name: "Save override" }),
		).toBeDisabled();
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
