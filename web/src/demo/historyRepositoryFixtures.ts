import type { AdminHistoryRepositoriesResponse } from "../api/adminHistoryRepositories";
import type { DemoNode } from "./types";

const JOURNAL_CAPACITY_BYTES = 128 * 1024 ** 2;

export function createDemoHistoryRepositoriesStatus(
	nodes: readonly DemoNode[],
): AdminHistoryRepositoriesResponse {
	return {
		configured: nodes.length > 0,
		partial: false,
		unreachable_node_ids: [],
		items: nodes.map((node, index) => {
			const capacityGuard = index === 0;
			const offline = node.status === "offline";
			const usedBytes = capacityGuard ? JOURNAL_CAPACITY_BYTES : 12 * 1024 ** 2;
			return {
				member: {
					identity: {
						node_id: node.id,
						ed25519_public_key: `${node.id}-signing-key`,
						x25519_relay_public_key: `${node.id}-relay-key`,
					},
					lifecycle: offline ? "syncing" : "ready",
					replica_converged: !offline,
					capacity: {
						quota_bytes: JOURNAL_CAPACITY_BYTES,
						used_bytes: usedBytes,
						filesystem_available_bytes: 10 * JOURNAL_CAPACITY_BYTES,
					},
				},
				runtime: offline
					? undefined
					: {
							storage_mode: "sqlite",
							capacity: {
								quota_bytes: JOURNAL_CAPACITY_BYTES,
								used_bytes: usedBytes,
								filesystem_available_bytes: 10 * JOURNAL_CAPACITY_BYTES,
							},
							record_count: capacityGuard ? 20_000 : 124,
							segment_count: capacityGuard ? 20_000 : 124,
							gap_count: 0,
							history_truncated: false,
							last_verified_unix_seconds: null,
							last_anti_entropy_unix_seconds: null,
							last_deep_verification_unix_seconds: null,
							last_dynamic_relay_attempt_unix_seconds: null,
							source_delivery: {
								state: capacityGuard ? "journal_capacity_guard" : "idle",
								pending_segments: capacityGuard ? 20_000 : 0,
								pending_bytes: capacityGuard ? JOURNAL_CAPACITY_BYTES : 0,
								oldest_pending_cursor: capacityGuard
									? `${node.id}/4/runtime/3993`
									: null,
								oldest_pending_age_seconds: null,
								last_acknowledged_at: null,
								last_delivery_path: null,
							},
						},
			};
		}),
	};
}
