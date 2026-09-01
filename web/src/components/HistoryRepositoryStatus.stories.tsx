import type { Meta, StoryObj } from "@storybook/react";

import type { AdminHistoryRepositoriesResponse } from "../api/adminHistoryRepositories";
import { fixtureCatalog } from "../fixture-policy/catalog";
import {
	RepositoryQueryQuality,
	RepositoryStatusSummary,
} from "./HistoryRepositoryStatus";

const healthy: AdminHistoryRepositoriesResponse = {
	configured: true,
	partial: false,
	unreachable_node_ids: [],
	items: [
		{
			member: {
				identity: {
					node_id: fixtureCatalog.nodeId.fixture17(),
					ed25519_public_key: "fixture",
					x25519_relay_public_key: "fixture",
				},
				lifecycle: "ready",
				catch_up_completed_at: 1_786_000_000,
				ready_at: 1_786_000_300,
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
				last_verified_unix_seconds: 1_786_001_200,
				last_anti_entropy_unix_seconds: 1_786_001_100,
				last_deep_verification_unix_seconds: 1_785_950_000,
				last_dynamic_relay_attempt_unix_seconds: null,
				source_delivery: {
					state: "idle",
					pending_segments: 0,
					pending_bytes: 0,
				},
			},
		},
	],
};

const meta = {
	title: "Components/HistoryRepositoryStatus",
	component: RepositoryStatusSummary,
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
	parameters: { layout: "padded" },
	args: { status: healthy },
} satisfies Meta<typeof RepositoryStatusSummary>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Healthy: Story = {};

export const SyncingWithGaps: Story = {
	args: {
		status: {
			...healthy,
			items: healthy.items.map((item) => ({
				...item,
				member: {
					...item.member,
					lifecycle: "syncing",
					replica_converged: false,
				},
				runtime: item.runtime
					? { ...item.runtime, gap_count: 3, history_truncated: true }
					: undefined,
			})),
		},
	},
};

export const SourceBacklogged: Story = {
	args: {
		status: {
			...healthy,
			items: healthy.items.map((item) => ({
				...item,
				runtime: item.runtime
					? {
							...item.runtime,
							gap_count: 0,
							source_delivery: {
								state: "backlogged",
								pending_segments: 37,
								pending_bytes: 128 * 1024,
								oldest_pending_cursor: "node-a/4/runtime/3993",
								oldest_pending_age_seconds: 900,
							},
						}
					: undefined,
			})),
		},
	},
};

export const SourceJournalOrderRepairing: Story = {
	args: {
		status: {
			...healthy,
			items: healthy.items.map((item) => ({
				...item,
				runtime: item.runtime
					? {
							...item.runtime,
							gap_count: 0,
							source_delivery: {
								state: "journal_order_repairing",
								pending_segments: 20_000,
								pending_bytes: 128 * 1024 * 1024,
								oldest_pending_age_seconds: 600,
							},
						}
					: undefined,
			})),
		},
	},
};

export const RemoteUnavailable: Story = {
	args: {
		status: {
			...healthy,
			partial: true,
			unreachable_node_ids: ["01K2REPOSITORY0000000000002"],
			items: [
				...healthy.items,
				{
					member: {
						...healthy.items[0].member,
						identity: {
							...healthy.items[0].member.identity,
							node_id: fixtureCatalog.nodeId.fixture32(),
						},
					},
				},
			],
		},
	},
};

export const RemoteUnavailableMobile: Story = {
	...RemoteUnavailable,
	parameters: {
		viewport: {
			defaultViewport: "historyRepositoryMobile",
			viewports: {
				historyRepositoryMobile: {
					name: "History repository mobile (393x852)",
					styles: { height: "852px", width: "393px" },
					type: "mobile",
				},
			},
		},
	},
};

export const Empty: Story = {
	args: {
		status: {
			configured: false,
			partial: false,
			unreachable_node_ids: [],
			items: [],
		},
	},
};

export const PartialQueryQuality: Story = {
	render: () => (
		<RepositoryQueryQuality
			history={{
				repository: "01K2REPOSITORY0000000000001",
				completeness: "partial",
				coverage: {
					observed: {
						start_unix_seconds: 1_785_913_600,
						end_unix_seconds: 1_786_000_000,
					},
					received: {
						start_unix_seconds: 1_785_999_400,
						end_unix_seconds: 1_786_000_000,
					},
				},
				watermarks: [
					{
						source_node_id: "01K2SOURCE000000000000000001",
						source_epoch: 4,
						stream: "traffic",
						sequence: 991,
					},
				],
				gaps: [
					{
						range: {
							start_unix_seconds: 1_785_999_400,
							end_unix_seconds: 1_786_000_000,
						},
						permanent: true,
					},
				],
				clock_skew_seconds: 42,
				records: [],
				records_truncated: true,
				next_page_cursor: "100",
			}}
		/>
	),
};
