import type { ReactNode } from "react";

import { Icon } from "./Icon";
import { badgeClass } from "./ui-helpers";
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
			className={badgeClass(
				"ghost",
				"sm",
				"font-mono transition-colors hover:bg-accent",
			)}
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
				className={badgeClass("ghost", "sm", "font-mono")}
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
				className={badgeClass(
					"warning",
					"sm",
					"font-mono transition hover:brightness-95",
				)}
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
				className={badgeClass("ghost", "sm", "font-mono")}
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
				className={badgeClass(
					"error",
					"sm",
					"cursor-pointer font-mono transition hover:brightness-95",
				)}
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
			className={badgeClass("ghost", "sm", "font-mono")}
			title="Focus the page to check updates (1h cooldown)"
		>
			<Icon name="tabler:refresh" size={14} className="opacity-70" />
			<span className="sr-only">update</span>
		</span>
	);
}
