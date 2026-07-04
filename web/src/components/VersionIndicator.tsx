import { type ReactNode, useMemo, useRef, useState } from "react";

import type { AdminUpgradeStatusResponse } from "../api/adminUpgrade";
import { Button } from "./Button";
import { ConfirmDialog } from "./ConfirmDialog";
import { Icon } from "./Icon";
import { badgeClass } from "./ui-helpers";
import { Popover, PopoverContent, PopoverTrigger } from "./ui/popover";
import {
	type VersionCheckUiState,
	XP_GITHUB_REPO,
	githubReleaseTagUrl,
	xpVersionLinkHref,
} from "./versionCheckUi";

type VersionIndicatorProps = {
	xpVersion: string | null | undefined;
	versionCheck: VersionCheckUiState;
	defaultOpen?: boolean;
	upgradeStatus?: AdminUpgradeStatusResponse | null;
	upgradeStatusError?: string | null;
	upgradeStatusLoading?: boolean;
	upgradeStarting?: boolean;
	onRetryVersionCheck?: () => void;
	onRefreshUpgradeStatus?: () => void;
	onStartUpgrade?: (targetTag: string, repo: string) => void;
};

export function VersionIndicator({
	xpVersion,
	versionCheck,
	defaultOpen = false,
	upgradeStatus,
	upgradeStatusError,
	upgradeStatusLoading = false,
	upgradeStarting = false,
	onRetryVersionCheck,
	onRefreshUpgradeStatus,
	onStartUpgrade,
}: VersionIndicatorProps) {
	const [open, setOpen] = useState(defaultOpen);
	const [confirmOpen, setConfirmOpen] = useState(false);
	const closeTimerRef = useRef<number | null>(null);
	const repo = versionRepo(versionCheck);
	const currentHref = xpVersionLinkHref(xpVersion, repo);
	const latestTag =
		versionCheck.kind === "update_available" ||
		versionCheck.kind === "up_to_date"
			? versionCheck.latest_tag
			: null;
	const latestHref = latestTag ? githubReleaseTagUrl(latestTag, repo) : null;
	const upgradeTarget =
		versionCheck.kind === "update_available" ? versionCheck.latest_tag : null;
	const job = upgradeStatus?.status ?? null;
	const support = upgradeStatus?.support ?? null;
	const jobActive = job?.state === "running" || job?.state === "restarting";
	const canUpgrade =
		Boolean(upgradeTarget) &&
		Boolean(support?.supported) &&
		!jobActive &&
		!upgradeStarting;
	const icon = indicatorIcon(versionCheck, job?.state);
	const tone = versionCheck.kind === "update_available" ? "warning" : "ghost";

	const statusText = useMemo(
		() => describeStatus(versionCheck, upgradeStatus, upgradeStatusError),
		[versionCheck, upgradeStatus, upgradeStatusError],
	);

	function clearCloseTimer() {
		if (closeTimerRef.current === null) return;
		window.clearTimeout(closeTimerRef.current);
		closeTimerRef.current = null;
	}

	function scheduleClose() {
		clearCloseTimer();
		closeTimerRef.current = window.setTimeout(() => setOpen(false), 160);
	}

	return (
		<>
			<Popover open={open} onOpenChange={setOpen}>
				<div
					onMouseEnter={() => {
						clearCloseTimer();
						setOpen(true);
					}}
					onMouseLeave={scheduleClose}
					onFocus={() => setOpen(true)}
					onBlur={scheduleClose}
				>
					<PopoverTrigger asChild>
						<button
							type="button"
							className={badgeClass(
								tone,
								"sm",
								[
									"cursor-pointer font-mono transition-colors",
									"hover:bg-accent focus-visible:ring-[3px]",
									"focus-visible:ring-ring/20",
								].join(" "),
							)}
							aria-label={statusText}
						>
							<span>xp</span>
							<span className="opacity-85">{xpVersion ?? "..."}</span>
							{icon}
						</button>
					</PopoverTrigger>
					<PopoverContent
						align="end"
						className="w-80 p-3"
						onMouseEnter={clearCloseTimer}
						onMouseLeave={scheduleClose}
					>
						<div className="space-y-3">
							<div className="flex items-start justify-between gap-3">
								<div className="min-w-0">
									<p className="text-sm font-semibold">xp version</p>
									<p className="mt-1 text-xs text-muted-foreground">
										{statusText}
									</p>
								</div>
								{versionCheck.kind === "checking" ? (
									<Icon
										name="tabler:loader-2"
										className="mt-0.5 animate-spin text-muted-foreground"
										size={16}
									/>
								) : null}
							</div>

							<div className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-2 text-xs">
								<span className="text-muted-foreground">Current</span>
								<a
									href={currentHref}
									target="_blank"
									rel="noreferrer"
									className="truncate font-mono text-foreground underline-offset-4 hover:underline"
								>
									{xpVersion ?? "unknown"}
								</a>
								<span className="text-muted-foreground">Latest</span>
								<span className="min-w-0 font-mono text-foreground">
									{latestHref ? (
										<a
											href={latestHref}
											target="_blank"
											rel="noreferrer"
											className="inline-flex min-w-0 items-center gap-1 underline-offset-4 hover:underline"
										>
											<span className="truncate">{latestTag}</span>
											<Icon name="tabler:external-link" size={13} />
										</a>
									) : (
										<span className="text-muted-foreground">not checked</span>
									)}
								</span>
								<span className="text-muted-foreground">Upgrade</span>
								<span className="min-w-0">{describeUpgrade(job, support)}</span>
							</div>

							{upgradeStatusError ? (
								<p className="rounded-lg bg-destructive/10 px-2.5 py-2 text-xs text-destructive">
									{upgradeStatusError}
								</p>
							) : null}

							<div className="flex items-center justify-between gap-2">
								<Button
									type="button"
									variant="ghost"
									size="sm"
									onClick={onRetryVersionCheck}
								>
									<Icon name="tabler:refresh" ariaLabel="Check" />
									Check
								</Button>
								<div className="flex items-center gap-2">
									<Button
										type="button"
										variant="ghost"
										size="sm"
										onClick={onRefreshUpgradeStatus}
										disabled={upgradeStatusLoading}
									>
										<Icon
											name={
												upgradeStatusLoading
													? "tabler:loader-2"
													: "tabler:activity"
											}
											className={
												upgradeStatusLoading ? "animate-spin" : undefined
											}
											ariaLabel="Refresh"
										/>
										Status
									</Button>
									<Button
										type="button"
										size="sm"
										disabled={!canUpgrade}
										onClick={() => setConfirmOpen(true)}
									>
										<Icon name="tabler:download" ariaLabel="Upgrade" />
										Upgrade
									</Button>
								</div>
							</div>
						</div>
					</PopoverContent>
				</div>
			</Popover>

			<ConfirmDialog
				open={confirmOpen}
				title="Upgrade this node?"
				description={
					upgradeTarget
						? [
								`This will start xp-ops upgrade to ${upgradeTarget}`,
								"on the current node. xp may restart and the UI may reconnect.",
							].join(" ")
						: "No upgrade target is available."
				}
				confirmLabel={upgradeStarting ? "Starting..." : "Start upgrade"}
				cancelLabel="Cancel"
				onCancel={() => setConfirmOpen(false)}
				onConfirm={() => {
					if (!upgradeTarget) return;
					setConfirmOpen(false);
					onStartUpgrade?.(upgradeTarget, repo);
				}}
			/>
		</>
	);
}

function versionRepo(versionCheck: VersionCheckUiState): string {
	return versionCheck.kind === "update_available" ||
		versionCheck.kind === "up_to_date"
		? versionCheck.repo
		: XP_GITHUB_REPO;
}

function indicatorIcon(
	versionCheck: VersionCheckUiState,
	jobState?: string,
): ReactNode {
	if (jobState === "running" || jobState === "restarting") {
		return <Icon name="tabler:loader-2" size={14} className="animate-spin" />;
	}
	if (versionCheck.kind === "checking") {
		return (
			<Icon
				name="tabler:loader-2"
				size={14}
				className="animate-spin opacity-70"
			/>
		);
	}
	if (versionCheck.kind === "update_available") {
		return <Icon name="tabler:download" size={14} className="opacity-90" />;
	}
	if (versionCheck.kind === "check_failed") {
		return <Icon name="tabler:refresh" size={14} className="opacity-80" />;
	}
	if (versionCheck.kind === "up_to_date" && versionCheck.comparable) {
		return <Icon name="tabler:circle-check" size={14} className="opacity-70" />;
	}
	return <Icon name="tabler:external-link" size={14} className="opacity-65" />;
}

function describeStatus(
	versionCheck: VersionCheckUiState,
	upgradeStatus?: AdminUpgradeStatusResponse | null,
	upgradeStatusError?: string | null,
): string {
	const job = upgradeStatus?.status;
	if (job?.state === "running" || job?.state === "restarting") {
		return `Upgrade to ${job.target_tag ?? "target release"} is running.`;
	}
	if (job?.state === "succeeded") {
		return `Last upgrade completed${job.target_tag ? ` to ${job.target_tag}` : ""}.`;
	}
	if (job?.state === "failed") {
		return job.message ?? "Last upgrade failed.";
	}
	if (upgradeStatusError) return "Upgrade status is unavailable.";
	if (versionCheck.kind === "update_available") {
		return `New version ${versionCheck.latest_tag} is available.`;
	}
	if (versionCheck.kind === "up_to_date") {
		return versionCheck.comparable
			? "This node is up to date."
			: `Latest is ${versionCheck.latest_tag}, but current version is not comparable.`;
	}
	if (versionCheck.kind === "checking") return "Checking latest release.";
	if (versionCheck.kind === "check_failed") return versionCheck.message;
	return "Focus the page to check for updates.";
}

function describeUpgrade(
	job: AdminUpgradeStatusResponse["status"] | null,
	support: AdminUpgradeStatusResponse["support"] | null,
): ReactNode {
	if (job?.state === "running" || job?.state === "restarting") {
		return <span className="text-warning">running</span>;
	}
	if (job?.state === "succeeded") {
		return <span className="text-success">last succeeded</span>;
	}
	if (job?.state === "failed") {
		return <span className="text-destructive">last failed</span>;
	}
	if (support && !support.supported) {
		return (
			<span className="text-muted-foreground">
				{support.reason ?? "not supported here"}
			</span>
		);
	}
	if (!support)
		return <span className="text-muted-foreground">sign in required</span>;
	return <span className="text-muted-foreground">ready</span>;
}
