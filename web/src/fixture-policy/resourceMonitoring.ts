import type { ResourceSnapshot } from "../api/adminResources";

import { fixtureCatalog } from "./catalog";

export const measurement = (
	value: number | undefined,
	capability: "supported" | "partial" | "unsupported" = "supported",
	reason_code?: string,
) => ({
	capability,
	...(value === undefined ? {} : { value }),
	...(reason_code ? { reason_code } : {}),
});

export const supportedResourceSnapshot: ResourceSnapshot = {
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
