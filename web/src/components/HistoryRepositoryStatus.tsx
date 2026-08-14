import type {
	AdminHistoryRepositoriesResponse,
	HistoryRepositoryMember,
	HistoryRepositoryRuntime,
} from "../api/adminHistoryRepositories";
import type { AdminRepositoryHistory } from "../api/adminRepositoryHistory";
import { Badge } from "./ui/badge";

function bytes(value: number): string {
	if (value >= 1024 ** 3) return `${(value / 1024 ** 3).toFixed(1)} GiB`;
	if (value >= 1024 ** 2) return `${(value / 1024 ** 2).toFixed(1)} MiB`;
	if (value >= 1024) return `${(value / 1024).toFixed(1)} KiB`;
	return `${value} B`;
}

function lifecycleVariant(lifecycle: HistoryRepositoryMember["lifecycle"]) {
	if (lifecycle === "ready") return "success" as const;
	if (lifecycle === "syncing") return "warning" as const;
	return "outline" as const;
}

function completenessVariant(
	completeness: AdminRepositoryHistory["completeness"],
) {
	if (completeness === "complete") return "success" as const;
	if (completeness === "partial") return "warning" as const;
	return "outline" as const;
}

function runtimeAvailability(runtime?: HistoryRepositoryRuntime): string {
	if (!runtime) return "offline";
	if (runtime.storage_mode !== "sqlite") return "degraded JSON";
	if (runtime.capacity.filesystem_available_bytes < 256 * 1024 * 1024) {
		return "low disk";
	}
	if (runtime.capacity.used_bytes >= runtime.capacity.quota_bytes) {
		return "quota reached";
	}
	if (runtime.history_truncated || runtime.gap_count > 0)
		return "gaps detected";
	return "healthy";
}

export function RepositoryMemberStatus(props: {
	member: HistoryRepositoryMember;
	runtime?: HistoryRepositoryRuntime;
	compact?: boolean;
}) {
	const { member, runtime, compact = false } = props;
	return (
		<div
			className={[
				"grid min-w-0 gap-2 border-t border-border/70 py-3",
				"sm:items-center sm:gap-4",
				"sm:[grid-template-columns:minmax(0,1fr)_auto_auto]",
			].join(" ")}
		>
			<div className="min-w-0">
				<p className="truncate font-mono text-xs">{member.identity.node_id}</p>
				<p className="mt-1 text-xs text-muted-foreground">
					{runtime
						? [
								runtimeAvailability(runtime),
								`${runtime.record_count} records`,
								`${runtime.gap_count} gaps`,
							].join(" · ")
						: "Repository runtime is unavailable."}
				</p>
			</div>
			<Badge variant={lifecycleVariant(member.lifecycle)}>
				{member.lifecycle}
			</Badge>
			{compact ? null : (
				<p className="font-mono text-xs text-muted-foreground sm:text-right">
					{runtime
						? `${bytes(runtime.capacity.used_bytes)} / ${bytes(runtime.capacity.quota_bytes)}`
						: "-"}
				</p>
			)}
		</div>
	);
}

export function RepositoryStatusSummary(props: {
	status: AdminHistoryRepositoriesResponse;
	title?: string;
	compact?: boolean;
}) {
	const { status, title = "History repositories", compact = false } = props;
	if (!status.configured) {
		return (
			<section className="border-t border-border/70 pt-4">
				<h2 className="text-lg font-semibold">{title}</h2>
				<p className="mt-2 text-sm text-muted-foreground">
					No history repository has been configured for this cluster.
				</p>
			</section>
		);
	}

	return (
		<section className="border-t border-border/70 pt-4">
			<div className="flex flex-wrap items-center justify-between gap-2">
				<h2 className="text-lg font-semibold">{title}</h2>
				{status.partial ? (
					<Badge variant="warning">partial</Badge>
				) : (
					<Badge variant="success">reachable</Badge>
				)}
			</div>
			{status.items.map((item) => (
				<RepositoryMemberStatus
					key={item.member.identity.node_id}
					member={item.member}
					runtime={item.runtime}
					compact={compact}
				/>
			))}
			{status.partial ? (
				<p className="mt-2 break-words text-xs text-muted-foreground">
					Unavailable: {status.unreachable_node_ids.join(", ")}
				</p>
			) : null}
		</section>
	);
}

export function RepositoryQueryQuality(props: {
	history: AdminRepositoryHistory;
}) {
	const { history } = props;
	return (
		<section className="border-l-2 border-border pl-3">
			<div className="flex flex-wrap items-center gap-2">
				<span className="text-sm font-medium">Repository history</span>
				<Badge variant={completenessVariant(history.completeness)}>
					{history.completeness}
				</Badge>
			</div>
			<p className="mt-1 break-words text-xs text-muted-foreground">
				{history.repository
					? `Source: ${history.repository}. `
					: "Source: local window. "}
				{history.gaps.length} gaps · skew {history.clock_skew_seconds}s
				{history.next_page_cursor ? " · more records available" : ""}
			</p>
		</section>
	);
}
