import type { Meta, StoryObj } from "@storybook/react";

import type { ResourceSnapshot } from "../api/adminResources";
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

const supportedSnapshot: ResourceSnapshot = {
	node_id: "node-tokyo-1",
	observed_at: "2026-09-01T11:00:00Z",
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
			state: "not_managed",
			capability: "partial",
			metrics: {
				cpu_percent: measurement(undefined, "unsupported", "not_managed"),
				rss_bytes: measurement(undefined, "unsupported", "not_managed"),
				pss_bytes: measurement(undefined, "unsupported", "not_managed"),
				read_bytes_per_second: measurement(
					undefined,
					"unsupported",
					"not_managed",
				),
				write_bytes_per_second: measurement(
					undefined,
					"unsupported",
					"not_managed",
				),
				fd_count: measurement(undefined, "unsupported", "not_managed"),
				thread_count: measurement(undefined, "unsupported", "not_managed"),
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
				total_bytes: 100 * 1024 ** 3,
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
	capture_state: "unsupported",
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
	decorators: [
		(Story) => (
			<div className="p-12">
				<div className="rounded border border-border bg-slate-800 p-6">
					<Story />
				</div>
			</div>
		),
	],
	parameters: {
		layout: "padded",
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

const historyPoints = Array.from({ length: 24 }, (_, index) => ({
	observed_at: `2026-09-01T10:${String(index).padStart(2, "0")}:00Z`,
	value: 30 + ((index * 7) % 35),
}));

export const Supported: Story = {
	args: { snapshot: supportedSnapshot, historyPoints },
};

export const PartialAndSuspended: Story = {
	args: { snapshot: partialSnapshot, historyPoints: historyPoints.slice(-8) },
};

export const SamplingGap: Story = {
	args: {
		snapshot: supportedSnapshot,
		historyPoints: historyPoints.map((point, index) =>
			index === 12 ? { ...point, value: null } : point,
		),
	},
};

export const Unsupported: Story = {
	args: { snapshot: unsupportedSnapshot, historyPoints: [] },
};

export const Loading: Story = {
	args: { snapshot: supportedSnapshot, historyPoints: [] },
	render: () => (
		<ResourceTabContent
			capabilityUnavailable={false}
			isLoading
			isError={false}
			error={null}
			isFetching
			isOnline
			onRetry={() => undefined}
			historyPoints={[]}
		/>
	),
};
