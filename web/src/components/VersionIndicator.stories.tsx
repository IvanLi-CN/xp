import type { Meta, StoryObj } from "@storybook/react";
import { expect, screen, userEvent } from "@storybook/test";
import type { ReactNode } from "react";
import { useState } from "react";
import { fixtureCatalog } from "../fixture-policy/catalog";

import type { AdminUpgradeStatusResponse } from "@/api/adminUpgrade";
import type { UpgradeObservation } from "@/offline/upgradeObservation";

import { VersionIndicator } from "./VersionIndicator";

const baseUpgradeStatus: AdminUpgradeStatusResponse = {
	support: {
		supported: true,
		reason: null,
		trigger: "systemd",
	},
	status: {
		state: "idle",
		target_tag: null,
		repo: null,
		started_at: null,
		finished_at: null,
		exit_code: null,
		message: null,
		updated_at: fixtureCatalog.slotString.s203(),
	},
};

const meta = {
	title: "Components/VersionIndicator",
	component: VersionIndicator,
	tags: ["autodocs", "coverage-ui"],
	args: {
		xpVersion: "0.1.0",
		versionCheck: { kind: "idle" as const },
		upgradeStatus: baseUpgradeStatus,
		onRetryVersionCheck: undefined,
		onRefreshUpgradeStatus: undefined,
		onStartUpgrade: undefined,
	},
	parameters: {
		docs: {
			description: {
				component: [
					"Shell version indicator with release status,",
					"interactive upgrade popover, support detection,",
					"and active upgrade states.",
				].join(" "),
			},
		},
	},
} satisfies Meta<typeof VersionIndicator>;

export default meta;

type Story = StoryObj<typeof meta>;

function Wrap(props: { children: ReactNode }) {
	return (
		<div className="flex min-h-56 items-start justify-end p-4">
			{props.children}
		</div>
	);
}

function EvidenceWrap(props: { children: ReactNode }) {
	return (
		<div className="flex min-h-[30rem] items-start justify-center p-12">
			{props.children}
		</div>
	);
}

function UpgradeObservationHarness() {
	const [observation, setObservation] = useState<UpgradeObservation | null>(
		null,
	);
	return (
		<Wrap>
			<VersionIndicator
				xpVersion="0.1.0"
				defaultOpen
				versionCheck={{
					kind: "update_available",
					latest_tag: "v0.2.0",
					checked_at: fixtureCatalog.slotString.s203(),
					repo: "IvanLi-CN/xp",
				}}
				upgradeStatus={baseUpgradeStatus}
				upgradeObservation={observation}
				onStartUpgrade={(targetTag) => {
					setObservation({
						targetTag,
						deadlineAtMs: Date.now() + 60_000,
						phase: "observing",
					});
				}}
			/>
		</Wrap>
	);
}

export const Idle: Story = {
	render: () => (
		<Wrap>
			<VersionIndicator
				xpVersion="0.1.0"
				versionCheck={{ kind: "idle" }}
				upgradeStatus={baseUpgradeStatus}
			/>
		</Wrap>
	),
};

export const Checking: Story = {
	render: () => (
		<Wrap>
			<VersionIndicator
				xpVersion="0.1.0"
				versionCheck={{ kind: "checking" }}
				upgradeStatus={baseUpgradeStatus}
			/>
		</Wrap>
	),
};

export const UpdateAvailable: Story = {
	render: () => (
		<Wrap>
			<VersionIndicator
				xpVersion="0.1.0"
				defaultOpen
				versionCheck={{
					kind: "update_available",
					latest_tag: "v0.2.0",
					checked_at: fixtureCatalog.slotString.s203(),
					repo: "IvanLi-CN/xp",
				}}
				upgradeStatus={baseUpgradeStatus}
				onRetryVersionCheck={() => {}}
				onRefreshUpgradeStatus={() => {}}
				onStartUpgrade={() => {}}
			/>
		</Wrap>
	),
};

export const UpdateAvailableUnsupported: Story = {
	render: () => (
		<Wrap>
			<VersionIndicator
				xpVersion="0.1.0"
				defaultOpen
				versionCheck={{
					kind: "update_available",
					latest_tag: "v0.2.0",
					checked_at: fixtureCatalog.slotString.s203(),
					repo: "IvanLi-CN/xp",
				}}
				upgradeStatus={{
					...baseUpgradeStatus,
					support: {
						supported: false,
						reason:
							"missing installed upgrade delegate; rerun xp-ops init on this host",
						trigger: null,
					},
					status: {
						...baseUpgradeStatus.status,
						state: "unsupported",
						message:
							"missing installed upgrade delegate; rerun xp-ops init on this host",
					},
				}}
				onRetryVersionCheck={() => {}}
				onRefreshUpgradeStatus={() => {}}
				onStartUpgrade={() => {}}
			/>
		</Wrap>
	),
};

export const UpdateAvailableCleansHistory: Story = {
	render: () => (
		<Wrap>
			<VersionIndicator
				xpVersion="0.1.0"
				defaultOpen
				versionCheck={{
					kind: "update_available",
					latest_tag: "v0.2.0",
					checked_at: fixtureCatalog.timestamp.baseline(),
					repo: "IvanLi-CN/xp",
				}}
				upgradeStatus={{
					...baseUpgradeStatus,
					support: {
						...baseUpgradeStatus.support,
						storage: {
							install: {
								path: "/usr/local/bin",
								available_bytes: 96 * 1024 * 1024,
								reclaimable_bytes: 48 * 1024 * 1024,
								required_bytes: 128 * 1024 * 1024,
								sufficient_after_cleanup: true,
							},
							workspace: {
								path: "/tmp/xp-ops",
								available_bytes: 256 * 1024 * 1024,
								reclaimable_bytes: 0,
								required_bytes: 128 * 1024 * 1024,
								sufficient_after_cleanup: true,
							},
							cleanup_required: true,
						},
					},
				}}
			/>
		</Wrap>
	),
};

export const UpdateAvailableInsufficientSpace: Story = {
	render: () => (
		<Wrap>
			<VersionIndicator
				xpVersion="0.1.0"
				defaultOpen
				versionCheck={{
					kind: "update_available",
					latest_tag: "v0.2.0",
					checked_at: fixtureCatalog.timestamp.baseline(),
					repo: "IvanLi-CN/xp",
				}}
				upgradeStatus={{
					...baseUpgradeStatus,
					support: {
						...baseUpgradeStatus.support,
						storage: {
							install: {
								path: "/usr/local/bin",
								available_bytes: 72 * 1024 * 1024,
								reclaimable_bytes: 8 * 1024 * 1024,
								required_bytes: 128 * 1024 * 1024,
								sufficient_after_cleanup: false,
							},
							workspace: {
								path: "/tmp/xp-ops",
								available_bytes: 256 * 1024 * 1024,
								reclaimable_bytes: 0,
								required_bytes: 128 * 1024 * 1024,
								sufficient_after_cleanup: true,
							},
							cleanup_required: true,
						},
					},
				}}
			/>
		</Wrap>
	),
};

export const UpdateAvailableInsufficientSpaceMobile: Story = {
	...UpdateAvailableInsufficientSpace,
	parameters: {
		viewport: {
			defaultViewport: "upgradeStorageMobile",
			viewports: {
				upgradeStorageMobile: {
					name: "Upgrade storage mobile (393x852)",
					styles: { height: "852px", width: "393px" },
					type: "mobile",
				},
			},
		},
	},
};

export const RunningUpgrade: Story = {
	render: () => (
		<Wrap>
			<VersionIndicator
				xpVersion="0.1.0"
				defaultOpen
				versionCheck={{
					kind: "update_available",
					latest_tag: "v0.2.0",
					checked_at: fixtureCatalog.slotString.s203(),
					repo: "IvanLi-CN/xp",
				}}
				upgradeStatus={{
					...baseUpgradeStatus,
					status: {
						...baseUpgradeStatus.status,
						state: "running",
						target_tag: "v0.2.0",
						repo: "IvanLi-CN/xp",
						started_at: fixtureCatalog.timestamp.baseline(),
						message: "xp-ops upgrade is running.",
					},
				}}
				onRetryVersionCheck={() => {}}
				onRefreshUpgradeStatus={() => {}}
				onStartUpgrade={() => {}}
			/>
		</Wrap>
	),
};

export const Reconnecting: Story = {
	render: () => (
		<Wrap>
			<VersionIndicator
				xpVersion="0.1.0"
				defaultOpen
				versionCheck={{
					kind: "update_available",
					latest_tag: "v0.2.0",
					checked_at: fixtureCatalog.slotString.s203(),
					repo: "IvanLi-CN/xp",
				}}
				upgradeStatus={baseUpgradeStatus}
				upgradeObservation={{
					targetTag: "v0.2.0",
					deadlineAtMs: Date.now() + 60_000,
					phase: "observing",
				}}
			/>
		</Wrap>
	),
};

export const StatusTimedOut: Story = {
	render: () => (
		<Wrap>
			<VersionIndicator
				xpVersion="0.1.0"
				defaultOpen
				versionCheck={{
					kind: "update_available",
					latest_tag: "v0.2.0",
					checked_at: fixtureCatalog.slotString.s203(),
					repo: "IvanLi-CN/xp",
				}}
				upgradeStatus={baseUpgradeStatus}
				upgradeObservation={{
					targetTag: "v0.2.0",
					deadlineAtMs: Date.now() - 1,
					phase: "timed_out",
				}}
			/>
		</Wrap>
	),
};

export const TerminalResult: Story = {
	render: () => (
		<Wrap>
			<VersionIndicator
				xpVersion="0.1.0"
				defaultOpen
				versionCheck={{
					kind: "update_available",
					latest_tag: "v0.2.0",
					checked_at: fixtureCatalog.slotString.s203(),
					repo: "IvanLi-CN/xp",
				}}
				upgradeStatus={{
					...baseUpgradeStatus,
					status: {
						...baseUpgradeStatus.status,
						state: "succeeded",
						target_tag: "v0.2.0",
						finished_at: fixtureCatalog.timestamp.recent(),
						exit_code: 0,
					},
				}}
				upgradeObservation={{
					targetTag: "v0.2.0",
					deadlineAtMs: 0,
					phase: "terminal",
				}}
			/>
		</Wrap>
	),
};

export const StaleUpgradeConflict: Story = {
	render: () => (
		<>
			<style>
				{`button[aria-label="Upgrade start conflicted with stale node status."] {
					visibility: hidden;
				}`}
			</style>
			<EvidenceWrap>
				<VersionIndicator
					xpVersion="0.1.0"
					defaultOpen
					versionCheck={{
						kind: "update_available",
						latest_tag: "v0.2.0",
						checked_at: "2026-07-04T00:00:00Z",
						repo: "IvanLi-CN/xp",
					}}
					upgradeStatus={baseUpgradeStatus}
					upgradeObservation={{
						targetTag: "v0.2.0",
						deadlineAtMs: Date.now() + 60_000,
						startedAtMs: Date.now(),
						phase: "conflict",
					}}
					onRefreshUpgradeStatus={() => {}}
					onStartUpgrade={() => {}}
				/>
			</EvidenceWrap>
		</>
	),
	play: async () => {
		await expect(
			await screen.findByText(/rejected the upgrade as already running/i),
		).toBeInTheDocument();
		await expect(
			await screen.findByRole("button", { name: "Upgrade" }),
		).toBeEnabled();
	},
};

export const ConfirmKeepsPopoverOpen: Story = {
	render: () => <UpgradeObservationHarness />,
	play: async () => {
		await userEvent.click(
			await screen.findByRole("button", { name: "Upgrade" }),
		);
		await userEvent.click(
			await screen.findByRole("button", { name: "Start upgrade" }),
		);
		await expect(
			await screen.findByText(
				"Waiting for the node to reconnect and report the upgrade status.",
			),
		).toBeInTheDocument();
		await expect(
			await screen.findByRole("button", { name: "Upgrading..." }),
		).toBeDisabled();
	},
};

export const UpgradeFailed: Story = {
	render: () => (
		<Wrap>
			<VersionIndicator
				xpVersion="0.1.0"
				defaultOpen
				versionCheck={{
					kind: "update_available",
					latest_tag: "v0.2.0",
					checked_at: fixtureCatalog.slotString.s203(),
					repo: "IvanLi-CN/xp",
				}}
				upgradeStatus={{
					...baseUpgradeStatus,
					status: {
						...baseUpgradeStatus.status,
						state: "failed",
						target_tag: "v0.2.0",
						repo: "IvanLi-CN/xp",
						started_at: fixtureCatalog.timestamp.baseline(),
						finished_at: fixtureCatalog.timestamp.recent(),
						exit_code: 7,
						message: "service_error: systemd start failed",
					},
				}}
				onRetryVersionCheck={() => {}}
				onRefreshUpgradeStatus={() => {}}
				onStartUpgrade={() => {}}
			/>
		</Wrap>
	),
};

export const UpToDateComparable: Story = {
	render: () => (
		<Wrap>
			<VersionIndicator
				xpVersion="0.1.0"
				versionCheck={{
					kind: "up_to_date",
					latest_tag: "v0.1.0",
					checked_at: fixtureCatalog.slotString.s203(),
					comparable: true,
					repo: "IvanLi-CN/xp",
				}}
				upgradeStatus={baseUpgradeStatus}
			/>
		</Wrap>
	),
};

export const UpToDateUncomparable: Story = {
	render: () => (
		<Wrap>
			<VersionIndicator
				xpVersion="main"
				versionCheck={{
					kind: "up_to_date",
					latest_tag: "main",
					checked_at: fixtureCatalog.slotString.s203(),
					comparable: false,
					repo: "IvanLi-CN/xp",
				}}
				upgradeStatus={baseUpgradeStatus}
			/>
		</Wrap>
	),
};

export const CheckFailed: Story = {
	render: () => (
		<Wrap>
			<VersionIndicator
				xpVersion="0.1.0"
				defaultOpen
				versionCheck={{ kind: "check_failed", message: "request failed: 502" }}
				upgradeStatusError="502 version_check_failed: GitHub release lookup failed"
				upgradeStatus={baseUpgradeStatus}
				onRetryVersionCheck={() => {}}
				onRefreshUpgradeStatus={() => {}}
			/>
		</Wrap>
	),
};
