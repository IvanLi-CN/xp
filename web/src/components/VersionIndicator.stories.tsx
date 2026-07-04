import type { Meta, StoryObj } from "@storybook/react";
import type { ReactNode } from "react";

import type { AdminUpgradeStatusResponse } from "@/api/adminUpgrade";
import { Card, CardContent } from "@/components/ui/card";

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
		updated_at: "2026-07-04T00:00:00Z",
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
		<Card>
			<CardContent className="flex min-h-56 items-start justify-end p-4">
				{props.children}
			</CardContent>
		</Card>
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
					checked_at: "2026-07-04T00:00:00Z",
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
					checked_at: "2026-07-04T00:00:00Z",
					repo: "IvanLi-CN/xp",
				}}
				upgradeStatus={{
					...baseUpgradeStatus,
					support: {
						supported: false,
						reason:
							"Web automatic upgrade is not supported inside the single-image container runtime.",
						trigger: null,
					},
					status: {
						...baseUpgradeStatus.status,
						state: "unsupported",
						message: "Use host-side image or Compose upgrade.",
					},
				}}
				onRetryVersionCheck={() => {}}
				onRefreshUpgradeStatus={() => {}}
				onStartUpgrade={() => {}}
			/>
		</Wrap>
	),
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
					checked_at: "2026-07-04T00:00:00Z",
					repo: "IvanLi-CN/xp",
				}}
				upgradeStatus={{
					...baseUpgradeStatus,
					status: {
						...baseUpgradeStatus.status,
						state: "running",
						target_tag: "v0.2.0",
						repo: "IvanLi-CN/xp",
						started_at: "2026-07-04T00:00:10Z",
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

export const UpgradeFailed: Story = {
	render: () => (
		<Wrap>
			<VersionIndicator
				xpVersion="0.1.0"
				defaultOpen
				versionCheck={{
					kind: "update_available",
					latest_tag: "v0.2.0",
					checked_at: "2026-07-04T00:00:00Z",
					repo: "IvanLi-CN/xp",
				}}
				upgradeStatus={{
					...baseUpgradeStatus,
					status: {
						...baseUpgradeStatus.status,
						state: "failed",
						target_tag: "v0.2.0",
						repo: "IvanLi-CN/xp",
						started_at: "2026-07-04T00:00:10Z",
						finished_at: "2026-07-04T00:00:45Z",
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
					checked_at: "2026-07-04T00:00:00Z",
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
					checked_at: "2026-07-04T00:00:00Z",
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
