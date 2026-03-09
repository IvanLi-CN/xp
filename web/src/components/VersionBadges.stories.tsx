import type { Meta, StoryObj } from "@storybook/react";
import type { ReactNode } from "react";

import { Card, CardContent } from "@/components/ui/card";

import { VersionBadges } from "./VersionBadges";

const meta = {
	title: "Components/VersionBadges",
	component: VersionBadges,
	tags: ["autodocs", "coverage-ui"],
	args: {
		xpVersion: "0.1.0",
		versionCheck: { kind: "idle" as const },
		onRetry: undefined,
	},
	parameters: {
		docs: {
			description: {
				component:
					"Release status badges used in the shell and settings surfaces. Stories cover idle, loading, update, comparable and non-comparable version checks, plus retry-able failures.",
			},
		},
	},
} satisfies Meta<typeof VersionBadges>;

export default meta;

type Story = StoryObj<typeof meta>;

function Wrap(props: { children: ReactNode }) {
	return (
		<Card>
			<CardContent className="flex items-center gap-2 p-4">
				{props.children}
			</CardContent>
		</Card>
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
	args: {
		versionCheck: { kind: "checking" },
	},
	render: () => (
		<Wrap>
			<VersionBadges xpVersion="0.1.0" versionCheck={{ kind: "checking" }} />
		</Wrap>
	),
};

export const UpdateAvailable: Story = {
	args: {
		versionCheck: {
			kind: "update_available",
			latest_tag: "v0.2.0",
			checked_at: "2026-01-31T00:00:00Z",
			repo: "IvanLi-CN/xp",
		},
	},
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
	args: {
		versionCheck: {
			kind: "up_to_date",
			latest_tag: "v0.1.0",
			checked_at: "2026-01-31T00:00:00Z",
			comparable: true,
			repo: "IvanLi-CN/xp",
		},
	},
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
	args: {
		xpVersion: "main",
		versionCheck: {
			kind: "up_to_date",
			latest_tag: "main",
			checked_at: "2026-01-31T00:00:00Z",
			comparable: false,
			repo: "IvanLi-CN/xp",
		},
	},
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
	args: {
		versionCheck: { kind: "check_failed", message: "request failed: 502" },
		onRetry: () => {},
	},
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
