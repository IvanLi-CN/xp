import type { AdminHistoryRepositoriesResponse } from "../../src/api/adminHistoryRepositories";
import type { AdminNode } from "../../src/api/adminNodes";
import type { AdminRepositoryHistory } from "../../src/api/adminRepositoryHistory";
import { fixtureCatalog } from "../../src/fixture-policy/catalog";

export function buildHistoryRepositories(
	nodes: AdminNode[],
): AdminHistoryRepositoriesResponse {
	if (!nodes[0]) {
		return {
			configured: false,
			partial: false,
			unreachable_node_ids: [],
			items: [],
		};
	}
	return {
		configured: true,
		partial: false,
		unreachable_node_ids: [],
		items: [
			{
				member: {
					identity: {
						node_id: nodes[0]
							? fixtureCatalog.nodeId.fixture32()
							: fixtureCatalog.nodeId.fixture17(),
						ed25519_public_key: "storybook-ed25519",
						x25519_relay_public_key: "storybook-x25519",
					},
					lifecycle: "ready",
					catch_up_completed_at: 1_783_024_000,
					ready_at: 1_783_024_300,
					replica_converged: true,
					capacity: {
						quota_bytes: 10 * 1024 ** 3,
						used_bytes: 2 * 1024 ** 3,
						filesystem_available_bytes: 48 * 1024 ** 3,
					},
				},
				runtime: {
					storage_mode: "sqlite",
					capacity: {
						quota_bytes: 10 * 1024 ** 3,
						used_bytes: 2 * 1024 ** 3,
						filesystem_available_bytes: 48 * 1024 ** 3,
					},
					record_count: 8_240,
					segment_count: 162,
					gap_count: 0,
					history_truncated: false,
					last_verified_unix_seconds: 1_783_024_600,
					last_anti_entropy_unix_seconds: 1_783_024_540,
					last_deep_verification_unix_seconds: 1_783_022_800,
					last_dynamic_relay_attempt_unix_seconds: null,
				},
			},
		],
	};
}

export function buildRepositoryHistory(
	nodes: AdminNode[],
): AdminRepositoryHistory {
	return {
		repository: nodes[0] ? fixtureCatalog.nodeId.fixture17() : null,
		completeness: "complete",
		coverage: {
			observed: {
				start_unix_seconds: 1_782_937_200,
				end_unix_seconds: 1_783_023_600,
			},
			received: {
				start_unix_seconds: 1_782_937_200,
				end_unix_seconds: 1_783_023_600,
			},
		},
		watermarks: [
			{
				source_node_id: nodes[0]
					? fixtureCatalog.nodeId.fixture17()
					: fixtureCatalog.nodeId.fixture32(),
				source_epoch: 1,
				stream: "traffic",
				sequence: 4_096,
			},
		],
		gaps: [],
		clock_skew_seconds: 2,
		records: [],
		records_truncated: false,
	};
}
