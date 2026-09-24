import type { Meta, StoryObj } from "@storybook/react";
import { expect, within } from "@storybook/test";
import type { ReactNode } from "react";

import type {
	NodeResourceHistoryMetric,
	ResourceSnapshot,
} from "../api/adminResources";
import { BackendApiError } from "../api/backendError";
import { fixtureCatalog } from "../fixture-policy/catalog";
import {
	ResourceSnapshotPanel,
	ResourceTabContent,
} from "./ResourceSnapshotPanel";

const measurement = (
	value: number | undefined,
	capability: "supported" | "partial" | "unsupported" = "supported",
	reason_code?: string,
) => ({
	capability,
	...(value === undefined ? {} : { value }),
	...(reason_code ? { reason_code } : {}),
});

export const supportedSnapshot: ResourceSnapshot = {
	node_id: fixtureCatalog.identifier.nodePrimary(),
	observed_at: fixtureCatalog.timestamp.t20260901T000000(),
	resource_domain: "host",
	capture_state: "active",
	capability: "supported",
	domain: {
		cpu_busy_percent: measurement(42.8),
		cpu_iowait_percent: measurement(1.2),
		load1: measurement(1.4),
		memory_total_bytes: measurement(16 * 1024 ** 3),
		memory_available_bytes: measurement(7.3 * 1024 ** 3),
		swap_total_bytes: measurement(2 * 1024 ** 3),
		swap_free_bytes: measurement(2 * 1024 ** 3),
		filesystems: [
			{
				mount: "/",
				capability: "supported",
				total_bytes: fixtureCatalog.quota.tenGiB(),
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
				cpu_percent: measurement(8.1),
				rss_bytes: measurement(48 * 1024 * 1024),
				pss_bytes: measurement(42 * 1024 * 1024),
				read_bytes_per_second: measurement(12 * 1024),
				write_bytes_per_second: measurement(8 * 1024),
				fd_count: measurement(32),
				thread_count: measurement(12),
			},
		},
		{
			role: "xray",
			state: "managed",
			capability: "supported",
			metrics: {
				cpu_percent: measurement(18.4),
				rss_bytes: measurement(96 * 1024 * 1024),
				pss_bytes: measurement(83 * 1024 * 1024),
				read_bytes_per_second: measurement(240 * 1024),
				write_bytes_per_second: measurement(76 * 1024),
				fd_count: measurement(188),
				thread_count: measurement(21),
			},
		},
		{
			role: "cloudflared",
			state: "managed",
			capability: "supported",
			metrics: {
				cpu_percent: measurement(3.2),
				rss_bytes: measurement(28 * 1024 * 1024),
				pss_bytes: measurement(24 * 1024 * 1024),
				read_bytes_per_second: measurement(96 * 1024),
				write_bytes_per_second: measurement(31 * 1024),
				fd_count: measurement(48),
				thread_count: measurement(9),
			},
		},
		{
			role: "canary",
			state: "managed",
			capability: "unsupported",
			metrics: {
				cpu_percent: measurement(
					undefined,
					"unsupported",
					"runtime_not_separable",
				),
				rss_bytes: measurement(
					undefined,
					"unsupported",
					"runtime_not_separable",
				),
				pss_bytes: measurement(
					undefined,
					"unsupported",
					"runtime_not_separable",
				),
				read_bytes_per_second: measurement(
					undefined,
					"unsupported",
					"runtime_not_separable",
				),
				write_bytes_per_second: measurement(
					undefined,
					"unsupported",
					"runtime_not_separable",
				),
				fd_count: measurement(
					undefined,
					"unsupported",
					"runtime_not_separable",
				),
				thread_count: measurement(
					undefined,
					"unsupported",
					"runtime_not_separable",
				),
			},
		},
	],
};

const partialSnapshot: ResourceSnapshot = {
	...supportedSnapshot,
	resource_domain: "cgroup",
	capture_state: "suspended",
	capability: "partial",
	domain: {
		...supportedSnapshot.domain,
		cpu_iowait_percent: measurement(
			undefined,
			"unsupported",
			"cgroup_iowait_unavailable",
		),
		filesystems: [
			{
				mount: "/",
				capability: "partial",
				total_bytes: fixtureCatalog.quota.tenGiB(),
				available_bytes: 8 * 1024 ** 3,
				used_percent: 92,
				used_inode_percent: 88,
			},
		],
	},
};

const unsupportedSnapshot: ResourceSnapshot = {
	...supportedSnapshot,
	resource_domain: "cgroup",
	capture_state: "active",
	capability: "unsupported",
	domain: {
		...supportedSnapshot.domain,
		cpu_busy_percent: measurement(
			undefined,
			"unsupported",
			"cgroup_cpu_unavailable",
		),
		cpu_iowait_percent: measurement(
			undefined,
			"unsupported",
			"cgroup_iowait_unavailable",
		),
		load1: measurement(undefined, "unsupported", "load_unavailable"),
		memory_available_bytes: measurement(
			undefined,
			"unsupported",
			"memory_unavailable",
		),
		filesystems: [],
	},
	runtimes: [],
};

const meta = {
	title: "Components/ResourceSnapshotPanel",
	component: ResourceSnapshotPanel,
	tags: ["autodocs", "coverage-ui"],
	parameters: {
		docs: {
			description: {
				component:
					"Bounded host/cgroup resource telemetry with explicit capability and quality states.",
			},
		},
	},
} satisfies Meta<typeof ResourceSnapshotPanel>;

export default meta;

type Story = StoryObj<typeof meta>;

const resourceHistoryBase =
	Date.parse(fixtureCatalog.timestamp.t20260901T000000()) + 10 * 60 * 60 * 1000;

function catalogTimestampOffset(index: number): string {
	return new Date(resourceHistoryBase + index * 60_000).toISOString();
}

const cpuHistoryPoints = Array.from({ length: 24 }, (_, index) => ({
	observed_at: catalogTimestampOffset(index),
	value: 30 + ((index * 7) % 35),
}));

const historyByMetric = {
	cpu_busy_percent: cpuHistoryPoints,
	memory_available_bytes: cpuHistoryPoints.map((point, index) => ({
		...point,
		value: (7 + ((index * 3) % 4) / 10) * 1024 ** 3,
	})),
	"filesystem.root.used_percent": cpuHistoryPoints.map((point, index) => ({
		...point,
		value: 34 + ((index * 5) % 9),
	})),
	cpu_iowait_percent: cpuHistoryPoints.map((point, index) => ({
		...point,
		value: 0.8 + ((index * 4) % 10) / 10,
	})),
} satisfies Record<
	NodeResourceHistoryMetric,
	Array<{ observed_at: string; value: number }>
>;

const runtimeHistoryByMetric = {
	cpu_percent: cpuHistoryPoints.map((point, index) => ({
		...point,
		value: 2 + ((index * 5) % 17),
	})),
	rss_bytes: cpuHistoryPoints.map((point, index) => ({
		...point,
		value: (42 + ((index * 3) % 9)) * 1024 ** 2,
	})),
	pss_bytes: cpuHistoryPoints.map((point, index) => ({
		...point,
		value: (36 + ((index * 2) % 8)) * 1024 ** 2,
	})),
	read_bytes_per_second: cpuHistoryPoints.map((point, index) => ({
		...point,
		value: (8 + ((index * 7) % 12)) * 1024,
	})),
	write_bytes_per_second: cpuHistoryPoints.map((point, index) => ({
		...point,
		value: (4 + ((index * 5) % 9)) * 1024,
	})),
	fd_count: cpuHistoryPoints.map((point, index) => ({
		...point,
		value: 24 + ((index * 3) % 12),
	})),
	thread_count: cpuHistoryPoints.map((point, index) => ({
		...point,
		value: 8 + ((index * 2) % 7),
	})),
};

function takeRecentHistory(pointCount: number) {
	return Object.fromEntries(
		Object.entries(historyByMetric).map(([metric, points]) => [
			metric,
			points.slice(-pointCount),
		]),
	);
}

function EvidenceFrame({ children }: { children: ReactNode }) {
	return (
		<div className="bg-background p-6" data-visual-evidence-surface>
			<div data-visual-evidence-target>{children}</div>
		</div>
	);
}

export const Supported: Story = {
	args: {
		snapshot: supportedSnapshot,
		historyByMetric,
		runtimeHistoryByMetric,
		selectedRuntimeRole: null,
		onRuntimeDetailsChange: () => undefined,
	},
};

export const PartialAndSuspended: Story = {
	args: {
		snapshot: partialSnapshot,
		historyByMetric: takeRecentHistory(8),
		runtimeHistoryByMetric: {},
		selectedRuntimeRole: null,
		onRuntimeDetailsChange: () => undefined,
	},
};

export const SamplingGap: Story = {
	args: {
		snapshot: supportedSnapshot,
		historyByMetric: {
			...historyByMetric,
			cpu_busy_percent: cpuHistoryPoints.map((point, index) =>
				index === 12 ? { ...point, value: null } : point,
			),
		},
		runtimeHistoryByMetric,
		selectedRuntimeRole: null,
		onRuntimeDetailsChange: () => undefined,
	},
};

export const Unsupported: Story = {
	args: {
		snapshot: unsupportedSnapshot,
		historyByMetric: {},
		runtimeHistoryByMetric: {},
		selectedRuntimeRole: null,
		onRuntimeDetailsChange: () => undefined,
	},
};

export const Loading: Story = {
	args: {
		snapshot: supportedSnapshot,
		historyByMetric: {},
		runtimeHistoryByMetric: {},
		selectedRuntimeRole: null,
		onRuntimeDetailsChange: () => undefined,
	},
	render: () => (
		<ResourceTabContent
			capabilityUnavailable={false}
			isLoading
			isError={false}
			error={null}
			isFetching
			isOnline
			onRetry={() => undefined}
			historyByMetric={{}}
			runtimeHistoryByMetric={{}}
			selectedRuntimeRole={null}
			onRuntimeDetailsChange={() => undefined}
		/>
	),
};

const resourceTabProps = {
	capabilityUnavailable: false,
	isLoading: false,
	isError: false,
	error: null,
	isFetching: false,
	isOnline: true,
	onRetry: () => undefined,
	historyByMetric,
	runtimeHistoryByMetric,
	selectedRuntimeRole: null,
	onRuntimeDetailsChange: () => undefined,
};

const resourcePanelStoryArgs = {
	snapshot: supportedSnapshot,
	historyByMetric: {},
	runtimeHistoryByMetric: {},
	selectedRuntimeRole: null,
	onRuntimeDetailsChange: () => undefined,
};

export const Offline: Story = {
	args: resourcePanelStoryArgs,
	render: () => (
		<EvidenceFrame>
			<ResourceTabContent
				{...resourceTabProps}
				isError
				isOnline={false}
				error={new TypeError("network request failed")}
				historyByMetric={{}}
				runtimeHistoryByMetric={{}}
			/>
		</EvidenceFrame>
	),
};

export const UnknownError: Story = {
	args: resourcePanelStoryArgs,
	render: () => (
		<EvidenceFrame>
			<ResourceTabContent
				{...resourceTabProps}
				isError
				error={new Error("unstructured backend detail must stay hidden")}
				historyByMetric={{}}
				runtimeHistoryByMetric={{}}
			/>
		</EvidenceFrame>
	),
	play: async ({ canvasElement }) => {
		await expect(
			await within(canvasElement).findByText("Resource read failed"),
		).toBeInTheDocument();
	},
};

export const XpApiError: Story = {
	args: resourcePanelStoryArgs,
	render: () => (
		<EvidenceFrame>
			<ResourceTabContent
				{...resourceTabProps}
				isError
				error={
					new BackendApiError({
						status: 500,
						code: "internal",
						message: "internal diagnostics are not shown",
						details: { failure_layer: "xp_api", retryable: true },
					})
				}
				historyByMetric={{}}
				runtimeHistoryByMetric={{}}
			/>
		</EvidenceFrame>
	),
};

export const PeerTransport: Story = {
	args: resourcePanelStoryArgs,
	render: () => (
		<EvidenceFrame>
			<ResourceTabContent
				{...resourceTabProps}
				isError
				error={
					new BackendApiError({
						status: 504,
						code: "peer_transport_timeout",
						message: "transport details are not shown",
						details: {
							failure_layer: "peer_transport",
							dispatch_state: "dispatched_no_verified_response",
							retryable: true,
						},
					})
				}
				historyByMetric={{}}
				runtimeHistoryByMetric={{}}
			/>
		</EvidenceFrame>
	),
};

export const PeerProtocol: Story = {
	args: resourcePanelStoryArgs,
	render: () => (
		<EvidenceFrame>
			<ResourceTabContent
				{...resourceTabProps}
				isError
				error={
					new BackendApiError({
						status: 502,
						code: "peer_protocol_rejected",
						message: "signature details are not shown",
						details: {
							failure_layer: "peer_protocol",
							cause: "protocol_rejected",
							retryable: true,
						},
					})
				}
				historyByMetric={{}}
				runtimeHistoryByMetric={{}}
			/>
		</EvidenceFrame>
	),
};

export const CircuitOpen: Story = {
	args: resourcePanelStoryArgs,
	parameters: {
		viewport: {
			defaultViewport: "resourceMobile",
			viewports: {
				resourceMobile: {
					name: "Resource mobile (393x852)",
					styles: { width: "393px", height: "852px" },
					type: "mobile",
				},
			},
		},
	},
	render: () => (
		<EvidenceFrame>
			<ResourceTabContent
				{...resourceTabProps}
				isError
				error={
					new BackendApiError({
						status: 503,
						code: "peer_circuit_open",
						message: "circuit details are not shown",
						details: {
							failure_layer: "circuit_breaker",
							dispatch_state: "not_dispatched",
							retryable: true,
							retry_after_seconds: 18,
							support_id: "01JRESOURCECIRCUIT",
						},
					})
				}
				historyByMetric={{}}
				runtimeHistoryByMetric={{}}
			/>
		</EvidenceFrame>
	),
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			await canvas.findByText("Peer path is cooling down"),
		).toBeInTheDocument();
		await expect(
			await canvas.findByRole("button", { name: /Retry resource read/i }),
		).toBeDisabled();
	},
};

export const RemoteError: Story = {
	args: resourcePanelStoryArgs,
	render: () => (
		<EvidenceFrame>
			<ResourceTabContent
				{...resourceTabProps}
				isError
				error={
					new BackendApiError({
						status: 429,
						code: "remote_node_error",
						message: "remote body is not shown",
						details: {
							failure_layer: "remote_node",
							target_status: 429,
							retryable: true,
						},
					})
				}
				historyByMetric={{}}
				runtimeHistoryByMetric={{}}
			/>
		</EvidenceFrame>
	),
};

export const StaleSnapshot: Story = {
	args: resourcePanelStoryArgs,
	render: () => (
		<EvidenceFrame>
			<ResourceTabContent
				{...resourceTabProps}
				dataUpdatedAt={Date.now() - 45 * 60 * 1000}
				isError
				error={
					new BackendApiError({
						status: 504,
						code: "peer_transport_timeout",
						message: "timeout details are not shown",
						details: { failure_layer: "peer_transport", retryable: true },
					})
				}
				snapshot={supportedSnapshot}
			/>
		</EvidenceFrame>
	),
	play: async ({ canvasElement }) => {
		await expect(
			await within(canvasElement).findByText(
				/Showing the last successful snapshot/,
			),
		).toBeInTheDocument();
	},
};

export const HistoryRefreshError: Story = {
	args: resourcePanelStoryArgs,
	render: () => (
		<EvidenceFrame>
			<ResourceTabContent
				{...resourceTabProps}
				isError={false}
				error={null}
				historyErrorByMetric={{
					cpu_busy_percent: new BackendApiError({
						status: 504,
						code: "peer_transport_timeout",
						message: "history details are not shown",
						details: { failure_layer: "peer_transport", retryable: true },
					}),
				}}
				snapshot={supportedSnapshot}
			/>
		</EvidenceFrame>
	),
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			await canvas.findByText("History unavailable"),
		).toBeInTheDocument();
		await expect(
			await canvas.findByRole("button", { name: "Retry history" }),
		).toBeInTheDocument();
	},
};
