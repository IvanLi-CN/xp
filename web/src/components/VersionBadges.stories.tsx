import type { Meta, StoryObj } from "@storybook/react";
import type { ReactNode } from "react";

import { VersionBadges } from "./VersionBadges";

const meta: Meta<typeof VersionBadges> = {
	title: "Components/VersionBadges",
	component: VersionBadges,
};

export default meta;

type Story = StoryObj<typeof VersionBadges>;

function Wrap(props: { children: ReactNode }) {
	return (
		<div className="rounded-box border border-base-200 bg-base-100 p-4">
			<div className="flex items-center gap-2">{props.children}</div>
		</div>
	);
}

export const Idle: Story = {
	render: () => (
		<Wrap>
			<VersionBadges xpVersion="0.1.0" versionCheck={{ kind: "idle" }} />
		</Wrap>
	),
};

export const Checking: Story = {
	render: () => (
		<Wrap>
			<VersionBadges xpVersion="0.1.0" versionCheck={{ kind: "checking" }} />
		</Wrap>
	),
};

export const UpdateAvailable: Story = {
	render: () => (
		<Wrap>
			<VersionBadges
				xpVersion="0.1.0"
				versionCheck={{
					kind: "update_available",
					latest_tag: "v0.2.0",
					checked_at: "2026-01-31T00:00:00Z",
					repo: "IvanLi-CN/xp",
				}}
			/>
		</Wrap>
	),
};

export const UpToDateComparable: Story = {
	render: () => (
		<Wrap>
			<VersionBadges
				xpVersion="0.1.0"
				versionCheck={{
					kind: "up_to_date",
					latest_tag: "v0.1.0",
					checked_at: "2026-01-31T00:00:00Z",
					comparable: true,
					repo: "IvanLi-CN/xp",
				}}
			/>
		</Wrap>
	),
};

export const UpToDateUncomparable: Story = {
	render: () => (
		<Wrap>
			<VersionBadges
				xpVersion="main"
				versionCheck={{
					kind: "up_to_date",
					latest_tag: "main",
					checked_at: "2026-01-31T00:00:00Z",
					comparable: false,
					repo: "IvanLi-CN/xp",
				}}
			/>
		</Wrap>
	),
};

export const CheckFailed: Story = {
	render: () => (
		<Wrap>
			<VersionBadges
				xpVersion="0.1.0"
				versionCheck={{ kind: "check_failed", message: "request failed: 502" }}
				onRetry={() => {}}
			/>
		</Wrap>
	),
};
