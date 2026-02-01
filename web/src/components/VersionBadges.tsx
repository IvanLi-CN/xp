import type { ReactNode } from "react";

import { Icon } from "./Icon";
import {
	type VersionCheckUiState,
	XP_GITHUB_REPO,
	githubReleaseTagUrl,
	xpVersionLinkHref,
} from "./versionCheckUi";

type VersionBadgesProps = {
	xpVersion: string | null | undefined;
	versionCheck: VersionCheckUiState;
	onRetry?: () => void;
};

export function VersionBadges({
	xpVersion,
	versionCheck,
	onRetry,
}: VersionBadgesProps): ReactNode[] {
	const repo =
		versionCheck.kind === "update_available" ||
		versionCheck.kind === "up_to_date"
			? versionCheck.repo
			: XP_GITHUB_REPO;
	const xpHref = xpVersionLinkHref(xpVersion, repo);

	const items: ReactNode[] = [];

	items.push(
		<a
			key="xp-version"
			href={xpHref}
			target="_blank"
			rel="noreferrer"
			className="badge badge-sm gap-2 font-mono badge-ghost hover:bg-base-200 transition-colors"
			title={
				xpVersion
					? `xp version (from /api/cluster/info): ${xpVersion}`
					: "xp version (from /api/cluster/info)"
			}
		>
			<span>xp</span>
			<span className="opacity-80">{xpVersion ?? "…"}</span>
			<Icon name="tabler:external-link" size={14} className="opacity-60" />
		</a>,
	);

	items.push(renderVersionCheckBadge(versionCheck, onRetry));

	return items;
}

function renderVersionCheckBadge(
	versionCheck: VersionCheckUiState,
	onRetry?: () => void,
): ReactNode {
	if (versionCheck.kind === "checking") {
		return (
			<span
				key="version-check"
				className="badge badge-sm gap-2 font-mono badge-ghost"
				title="Checking latest version…"
			>
				<Icon
					name="tabler:loader-2"
					size={14}
					className="animate-spin opacity-70"
				/>
				<span className="sr-only">checking</span>
			</span>
		);
	}

	if (versionCheck.kind === "update_available") {
		const href = githubReleaseTagUrl(
			versionCheck.latest_tag,
			versionCheck.repo,
		);
		return (
			<a
				key="version-check"
				href={href}
				target="_blank"
				rel="noreferrer"
				className="badge badge-sm gap-2 font-mono badge-warning text-warning-content hover:brightness-95 transition"
				title="Update available"
			>
				<Icon name="tabler:download" size={14} className="opacity-90" />
				<span className="font-semibold">{versionCheck.latest_tag}</span>
				<Icon name="tabler:external-link" size={14} className="opacity-90" />
			</a>
		);
	}

	if (versionCheck.kind === "up_to_date") {
		return (
			<span
				key="version-check"
				className="badge badge-sm gap-2 font-mono badge-ghost"
				title={
					versionCheck.comparable
						? `Up to date (checked at ${versionCheck.checked_at})`
						: `Latest is ${versionCheck.latest_tag}, but current version is not comparable`
				}
			>
				<Icon name="tabler:circle-check" size={14} className="opacity-70" />
				<span className="sr-only">
					{versionCheck.comparable ? "up-to-date" : "unknown"}
				</span>
			</span>
		);
	}

	if (versionCheck.kind === "check_failed") {
		return (
			<button
				key="version-check"
				type="button"
				className="badge badge-sm gap-2 font-mono badge-error hover:brightness-95 transition"
				title={versionCheck.message}
				onClick={onRetry}
			>
				<Icon name="tabler:refresh" size={14} className="opacity-80" />
				<span className="sr-only">retry</span>
			</button>
		);
	}

	return (
		<span
			key="version-check"
			className="badge badge-sm gap-2 font-mono badge-ghost"
			title="Focus the page to check updates (1h cooldown)"
		>
			<Icon name="tabler:refresh" size={14} className="opacity-70" />
			<span className="sr-only">update</span>
		</span>
	);
}
