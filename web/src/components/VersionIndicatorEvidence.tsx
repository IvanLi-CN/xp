import type { AdminUpgradeStatusResponse } from "@/api/adminUpgrade";
import { Card, CardContent } from "@/components/ui/card";
import type { ReactNode } from "react";

import { VersionIndicator } from "./VersionIndicator";

const baseUpgradeStatus: AdminUpgradeStatusResponse = {
	support: { supported: true, reason: null, trigger: "systemd" },
	status: {
		state: "idle",
		target_tag: null,
		repo: null,
		started_at: null,
		finished_at: null,
		exit_code: null,
		message: null,
		updated_at: "2026-08-03T00:00:00Z",
	},
};

const versionCheck = {
	kind: "update_available" as const,
	latest_tag: "v0.2.0",
	checked_at: "2026-08-03T00:00:00Z",
	repo: "IvanLi-CN/xp",
};

function EvidenceFrame(props: { children: ReactNode }) {
	return (
		<Card>
			<CardContent className="flex min-h-56 items-start justify-end p-4">
				{props.children}
			</CardContent>
		</Card>
	);
}

export function ReconnectingEvidence() {
	return (
		<EvidenceFrame>
			<VersionIndicator
				xpVersion="0.1.0"
				defaultOpen
				versionCheck={versionCheck}
				upgradeStatus={baseUpgradeStatus}
				upgradeObservation={{
					targetTag: "v0.2.0",
					deadlineAtMs: Date.now() + 60_000,
					phase: "observing",
				}}
			/>
		</EvidenceFrame>
	);
}

export function StatusTimedOutEvidence() {
	return (
		<EvidenceFrame>
			<VersionIndicator
				xpVersion="0.1.0"
				defaultOpen
				versionCheck={versionCheck}
				upgradeStatus={baseUpgradeStatus}
				upgradeObservation={{
					targetTag: "v0.2.0",
					deadlineAtMs: Date.now() - 1,
					phase: "timed_out",
				}}
			/>
		</EvidenceFrame>
	);
}
