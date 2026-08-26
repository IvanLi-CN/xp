import type {
	AdminHistoryRepositoriesResponse,
	HistoryRepositoryMember,
	HistoryRepositoryRuntime,
} from "../api/adminHistoryRepositories";
import type { AdminRepositoryHistory } from "../api/adminRepositoryHistory";
import { Button } from "./Button";
import { Badge } from "./ui/badge";

function bytes(value: number): string {
	if (value >= 1024 ** 3) return `${(value / 1024 ** 3).toFixed(1)} GiB`;
	if (value >= 1024 ** 2) return `${(value / 1024 ** 2).toFixed(1)} MiB`;
	if (value >= 1024) return `${(value / 1024).toFixed(1)} KiB`;
	return `${value} B`;
}

function fingerprint(publicKey: string): string {
	if (publicKey.length <= 18) return publicKey;
	return `${publicKey.slice(0, 10)}...${publicKey.slice(-8)}`;
}

function timestamp(value?: number): string {
	return value ? new Date(value * 1000).toLocaleString() : "pending";
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
	if (runtime.source_delivery?.state === "source_storage_guard") {
		return "source capture paused";
	}
	if (runtime.source_delivery?.state === "journal_unavailable") {
		return "source journal unavailable";
	}
	if (runtime.source_delivery?.state === "backlogged") {
		return "source backlog";
	}
	if (runtime.storage_mode === "sqlite_degraded")
		return "SQLite maintenance degraded";
	if (runtime.storage_mode === "degraded_json") return "JSON fallback";
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

function timeRange(range: {
	start_unix_seconds: number;
	end_unix_seconds: number;
}): string {
	const start = new Date(range.start_unix_seconds * 1000).toLocaleString();
	const end = new Date(range.end_unix_seconds * 1000).toLocaleString();
	return start === end ? start : `${start} - ${end}`;
}

function watermarkLabel(
	watermark: AdminRepositoryHistory["watermarks"][number],
) {
	return [
		watermark.source_node_id,
		`${watermark.stream}@${watermark.source_epoch}:${watermark.sequence}`,
	].join("/");
}

export function RepositoryMemberStatus(props: {
	member: HistoryRepositoryMember;
	runtime?: HistoryRepositoryRuntime;
	compact?: boolean;
	nodeName?: string;
}) {
	const { member, runtime, compact = false, nodeName } = props;
	const nodeId = member.identity.node_id;
	return (
		<div
			className={[
				"grid min-w-0 gap-2 border-t border-border/70 py-3",
				"sm:items-center sm:gap-4",
				"sm:[grid-template-columns:minmax(0,1fr)_auto_auto]",
			].join(" ")}
		>
			<div className="min-w-0">
				{nodeName ? <p className="truncate font-medium">{nodeName}</p> : null}
				<p className="break-all font-mono text-xs" title={nodeId}>
					{nodeId}
				</p>
				<p className="mt-1 text-xs text-muted-foreground">
					{runtime
						? [
								runtimeAvailability(runtime),
								`${runtime.record_count} records`,
								`${runtime.gap_count} gaps`,
							].join(" · ")
						: "Repository runtime is unavailable."}
				</p>
				{compact ? null : (
					<dl className="mt-2 grid min-w-0 gap-x-4 gap-y-1 text-xs sm:grid-cols-2">
						<div className="min-w-0">
							<dt className="text-muted-foreground">Signing public key</dt>
							<dd
								className="break-all font-mono"
								title={member.identity.ed25519_public_key}
							>
								{fingerprint(member.identity.ed25519_public_key)}
							</dd>
						</div>
						<div className="min-w-0">
							<dt className="text-muted-foreground">Relay public key</dt>
							<dd
								className="break-all font-mono"
								title={member.identity.x25519_relay_public_key}
							>
								{fingerprint(member.identity.x25519_relay_public_key)}
							</dd>
						</div>
						<div className="min-w-0">
							<dt className="text-muted-foreground">Caught up</dt>
							<dd>{timestamp(member.catch_up_completed_at)}</dd>
						</div>
						<div className="min-w-0">
							<dt className="text-muted-foreground">Ready</dt>
							<dd>{timestamp(member.ready_at)}</dd>
						</div>
						{runtime?.source_delivery?.state === "backlogged" ? (
							<div className="min-w-0 sm:col-span-2">
								<dt className="text-muted-foreground">Source backlog</dt>
								<dd>
									{runtime.source_delivery.pending_segments} segments ·{" "}
									{bytes(runtime.source_delivery.pending_bytes)}
									{runtime.source_delivery.oldest_pending_cursor
										? ` · from ${runtime.source_delivery.oldest_pending_cursor}`
										: ""}
								</dd>
							</div>
						) : null}
					</dl>
				)}
			</div>
			<div className="flex flex-wrap items-center gap-1 sm:justify-end">
				<Badge variant={lifecycleVariant(member.lifecycle)}>
					{member.lifecycle}
				</Badge>
				<Badge variant={member.replica_converged ? "success" : "warning"}>
					{member.replica_converged ? "converged" : "reconciling"}
				</Badge>
			</div>
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
	nodeNames?: Readonly<Record<string, string>>;
	showTitle?: boolean;
}) {
	const {
		status,
		title = "History repositories",
		compact = false,
		nodeNames,
		showTitle = true,
	} = props;
	if (!status.configured) {
		return (
			<section className="border-t border-border/70 pt-4">
				{showTitle ? <h2 className="text-lg font-semibold">{title}</h2> : null}
				<p className="mt-2 text-sm text-muted-foreground">
					No history repository has been configured for this cluster.
				</p>
			</section>
		);
	}

	return (
		<section className="border-t border-border/70 pt-4">
			<div className="flex flex-wrap items-center justify-between gap-2">
				{showTitle ? <h2 className="text-lg font-semibold">{title}</h2> : null}
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
					nodeName={
						nodeNames
							? (nodeNames[item.member.identity.node_id] ?? "Unknown node")
							: undefined
					}
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
	onNextPage?: () => void;
	nextPageLoading?: boolean;
}) {
	const { history, onNextPage, nextPageLoading = false } = props;
	const watermarks = history.watermarks.slice(0, 3);
	const gaps = history.gaps.slice(0, 2);
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
				{history.records.length} records · {history.gaps.length} gaps · skew{" "}
				{history.clock_skew_seconds}s
			</p>
			<dl className="mt-3 grid gap-2 text-xs sm:grid-cols-2">
				<div className="min-w-0">
					<dt className="text-muted-foreground">Observed coverage</dt>
					<dd className="break-words font-mono">
						{history.coverage
							? timeRange(history.coverage.observed)
							: "unavailable"}
					</dd>
				</div>
				<div className="min-w-0">
					<dt className="text-muted-foreground">Received coverage</dt>
					<dd className="break-words font-mono">
						{history.coverage
							? timeRange(history.coverage.received)
							: "unavailable"}
					</dd>
				</div>
				<div className="min-w-0 sm:col-span-2">
					<dt className="text-muted-foreground">Watermarks</dt>
					<dd className="break-words font-mono">
						{watermarks.length === 0
							? "none"
							: watermarks.map(watermarkLabel).join(", ")}
						{history.watermarks.length > watermarks.length
							? ` +${history.watermarks.length - watermarks.length}`
							: ""}
					</dd>
				</div>
				<div className="min-w-0 sm:col-span-2">
					<dt className="text-muted-foreground">Gaps</dt>
					<dd className="break-words font-mono">
						{gaps.length === 0
							? "none"
							: gaps
									.map(
										(gap) =>
											`${gap.permanent ? "permanent" : "repairing"}${
												gap.reason ? ` (${gap.reason})` : ""
											}: ${timeRange(gap.range)}`,
									)
									.join("; ")}
						{history.gaps.length > gaps.length
							? ` +${history.gaps.length - gaps.length}`
							: ""}
					</dd>
				</div>
			</dl>
			{onNextPage ? (
				<div className="mt-3">
					<Button
						variant="secondary"
						size="sm"
						loading={nextPageLoading}
						onClick={onNextPage}
					>
						Next page
					</Button>
				</div>
			) : null}
		</section>
	);
}
