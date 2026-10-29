import { type ReactNode, useState } from "react";

import type { AdminMeshStatus, MeshPreflightFailure } from "@/api/adminMesh";
import { Button } from "@/components/Button";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { Icon } from "@/components/Icon";
import {
	AlertDialogCancel,
	AlertDialogFooter,
} from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
import type { MeshMutationLock } from "@/utils/meshMutationLock";

export type MeshMutationPhase =
	| "idle"
	| "refreshing"
	| "confirming"
	| "submitting";

export type MeshMutationIssue = {
	kind:
		| "preflight"
		| "rejected"
		| "unknown"
		| "success_unconfirmed"
		| "persistence";
	message: string;
};

type MeshClusterSettingProps = {
	status?: AdminMeshStatus;
	statusUpdatedAt: number | null;
	statusFresh: boolean;
	statusRefreshing?: boolean;
	statusRefreshFailed?: boolean;
	observerBackend: string;
	adminTokenPresent: boolean;
	capabilityAvailable: boolean;
	capabilityReason?: string | null;
	isOnline: boolean;
	isReadOnly: boolean;
	mutationPhase: MeshMutationPhase;
	pendingTarget: boolean | null;
	currentConfirmationValue?: boolean;
	mutationIssue?: MeshMutationIssue | null;
	preflightFailures?: MeshPreflightFailure[];
	unknownLock?: MeshMutationLock | null;
	statusUnavailableLock?: MeshMutationLock | null;
	onRequestChange: (enabled: boolean) => void;
	onConfirm: () => void;
	onCancel: () => void;
	onRefresh: () => void;
	onUnlock: () => void;
};

function formatTimestamp(value: number | string | null | undefined): string {
	if (value === null || value === undefined) return "Unknown time";
	const timestamp = typeof value === "number" ? value : Date.parse(value);
	if (!Number.isFinite(timestamp)) return "Unknown time";
	return new Intl.DateTimeFormat(undefined, {
		year: "numeric",
		month: "short",
		day: "numeric",
		hour: "2-digit",
		minute: "2-digit",
	}).format(timestamp);
}

function clusterSettingLabel(value: boolean | undefined): string {
	return value === true ? "Enabled" : value === false ? "Disabled" : "Unknown";
}

const metadataClassName = [
	"mt-4 flex flex-wrap items-center",
	"gap-x-3 gap-y-1 text-xs text-muted-foreground",
].join(" ");

function pathSummary(status?: AdminMeshStatus) {
	const counts = { mesh: 0, public: 0, unknown: 0 };
	for (const peer of status?.peers ?? []) {
		if (peer.current_path === "mesh") counts.mesh += 1;
		else if (peer.current_path === "public") counts.public += 1;
		else counts.unknown += 1;
	}
	return counts;
}

function issueTone(issue: MeshMutationIssue["kind"]): "warning" | "error" {
	return issue === "unknown" || issue === "persistence" ? "warning" : "error";
}

function FailureList({
	failures,
	status,
}: {
	failures: MeshPreflightFailure[];
	status?: AdminMeshStatus;
}) {
	const names = new Map<string, string>();
	if (status) {
		names.set(status.local.node_id, status.local.node_name);
		for (const peer of status.peers) names.set(peer.node_id, peer.node_name);
	}
	return (
		<ul className="mt-2 space-y-1 text-sm">
			{failures.map((failure, index) => (
				<li
					key={`${failure.sender_node_id}-${failure.target_node_id}-${index}`}
					className="font-mono text-xs"
				>
					{names.get(failure.sender_node_id) ?? failure.sender_node_id} →{" "}
					{names.get(failure.target_node_id) ?? failure.target_node_id}{" "}
					<span className="font-sans text-muted-foreground">
						({failure.kind})
					</span>
				</li>
			))}
		</ul>
	);
}

function InfoCard({
	title,
	value,
	description,
	children,
}: {
	title: string;
	value: string;
	description: string;
	children?: ReactNode;
}) {
	return (
		<div className="rounded-2xl border border-border/70 bg-card px-4 py-4 shadow-sm">
			<div className="flex items-start justify-between gap-3">
				<div className="min-w-0">
					<h3 className="font-semibold">{title}</h3>
					<p className="mt-1 text-sm text-muted-foreground">{description}</p>
				</div>
				<Badge
					variant="outline"
					className="shrink-0 whitespace-nowrap text-center"
				>
					{value}
				</Badge>
			</div>
			{children ? <div className="mt-3">{children}</div> : null}
		</div>
	);
}

export function MeshClusterSetting({
	status,
	statusUpdatedAt,
	statusFresh,
	statusRefreshing = false,
	statusRefreshFailed = false,
	observerBackend,
	adminTokenPresent,
	capabilityAvailable,
	capabilityReason,
	isOnline,
	isReadOnly,
	mutationPhase,
	pendingTarget,
	currentConfirmationValue,
	mutationIssue,
	preflightFailures = [],
	unknownLock,
	statusUnavailableLock,
	onRequestChange,
	onConfirm,
	onCancel,
	onRefresh,
	onUnlock,
}: MeshClusterSettingProps) {
	const [unlockConfirmOpen, setUnlockConfirmOpen] = useState(false);
	const value = status?.cluster_mesh_enabled;
	const counts = status ? pathSummary(status) : null;
	const observerDataIncomplete =
		!statusFresh ||
		status?.peers.some(
			(peer) =>
				peer.stale ||
				peer.current_path === null ||
				peer.last_sample_at === null,
		) === true;
	const lock = unknownLock ?? statusUnavailableLock;
	const isLocked = lock !== undefined && lock !== null;
	const canEdit =
		statusFresh &&
		value !== undefined &&
		adminTokenPresent &&
		capabilityAvailable &&
		isOnline &&
		!isReadOnly &&
		!isLocked &&
		!statusRefreshFailed &&
		mutationIssue?.kind !== "unknown" &&
		mutationIssue?.kind !== "persistence" &&
		mutationPhase === "idle";
	const targetAlreadyApplied =
		pendingTarget !== null && currentConfirmationValue === pendingTarget;
	const isSubmitting = mutationPhase === "submitting";
	const statusReason = !adminTokenPresent
		? "Administrator authentication is required."
		: !capabilityAvailable
			? (capabilityReason ??
				"The connected API does not support Mesh configuration.")
			: !isOnline || isReadOnly
				? "Offline read-only mode blocks cluster mutations."
				: isLocked
					? "The previous mutation result is unknown. Verify it independently before unlocking."
					: statusRefreshFailed
						? "Refresh the current cluster state before changing this setting."
						: !statusFresh || value === undefined
							? "A fresh cluster setting snapshot is required before changing it."
							: mutationIssue?.kind === "success_unconfirmed"
								? mutationIssue.message
								: null;

	return (
		<section
			aria-labelledby="mesh-cluster-setting-heading"
			className="space-y-3"
		>
			<div>
				<div className="flex flex-wrap items-center gap-2">
					<h3
						id="mesh-cluster-setting-heading"
						className="text-base font-semibold"
					>
						Reality Direct Mesh
					</h3>
					<Badge variant={value === undefined ? "outline" : "secondary"}>
						{clusterSettingLabel(value)}
					</Badge>
				</div>
				<p className="mt-1 text-sm text-muted-foreground">
					One cluster-wide setting for signed control-plane Direct Mesh.
					Endpoint transport remains configured separately.
				</p>
			</div>

			<div className="grid gap-4 lg:grid-cols-[1.15fr_0.85fr]">
				<div className="rounded-2xl border border-border/70 bg-card px-4 py-4 shadow-sm">
					<div className="flex items-start justify-between gap-4">
						<div className="min-w-0">
							<p className="font-semibold">Enable Reality Direct Mesh</p>
							<p className="mt-1 text-sm text-muted-foreground">
								Applies to the whole cluster. Enabling validates directed voter
								paths; disabling uses the registered Public Path. Both changes
								require Mesh configuration support from all current voters and
								learners.
							</p>
						</div>
						<Switch
							aria-label="Enable Reality Direct Mesh"
							checked={value === true}
							className="mt-0.5"
							disabled={!canEdit}
							indeterminate={value === undefined}
							onCheckedChange={onRequestChange}
						/>
					</div>
					<div className={metadataClassName}>
						<span>Observed: {clusterSettingLabel(value)}</span>
						<span>Snapshot: {formatTimestamp(statusUpdatedAt)}</span>
						<span>Backend: {observerBackend || "current page"}</span>
					</div>
					{statusReason ? (
						<p className="mt-3 flex items-start gap-2 text-sm text-warning">
							<Icon
								name="tabler:alert-circle"
								size={16}
								className="mt-0.5 shrink-0"
							/>
							<span>{statusReason}</span>
						</p>
					) : null}
					{lock ? (
						<div className="mt-4 rounded-xl border border-warning/40 bg-warning/10 p-3 text-sm">
							<p className="font-medium text-warning-foreground">
								Mutation result unknown
							</p>
							<p className="mt-1 text-muted-foreground">
								A request to {lock.targetEnabled ? "enable" : "disable"} Mesh
								may still be in flight or may have been applied. Status
								observations do not prove its terminal result.
							</p>
							<div className="mt-3 flex flex-wrap gap-2">
								<Button
									variant="secondary"
									size="sm"
									onClick={onRefresh}
									loading={statusRefreshing}
								>
									Refresh observation
								</Button>
								<Button
									variant="outline"
									size="sm"
									onClick={() => setUnlockConfirmOpen(true)}
								>
									Verify and unlock
								</Button>
							</div>
						</div>
					) : null}
					{mutationIssue && !isLocked ? (
						<div
							role="alert"
							className={`mt-4 rounded-xl border p-3 text-sm ${
								issueTone(mutationIssue.kind) === "warning"
									? "border-warning/40 bg-warning/10"
									: "border-destructive/40 bg-destructive/10"
							}`}
						>
							<p className="font-medium">{mutationIssue.message}</p>
							{preflightFailures.length > 0 ? (
								<FailureList failures={preflightFailures} status={status} />
							) : null}
							{mutationIssue.kind === "preflight" ? (
								<p className="mt-3 text-muted-foreground">
									Review the observer and failed direction in{" "}
									<a
										href="/system-status"
										className="font-medium text-foreground underline underline-offset-4"
									>
										System status
									</a>
									, check the endpoint and canary, and run the existing probe if
									needed. A probe is diagnostic only; it does not prove the full
									cluster preflight has recovered.
								</p>
							) : null}
							<div className="mt-3 flex flex-wrap gap-2">
								<Button
									variant="secondary"
									size="sm"
									onClick={onRefresh}
									loading={statusRefreshing}
								>
									Refresh observation
								</Button>
								{mutationIssue.kind === "preflight" ? (
									<Button
										variant="outline"
										size="sm"
										onClick={() =>
											onRequestChange(pendingTarget ?? value !== true)
										}
									>
										Review and retry
									</Button>
								) : null}
							</div>
						</div>
					) : null}
				</div>

				<div className="space-y-4">
					<InfoCard
						title="Public Path"
						value="Supported fallback"
						description={
							"Registered public HTTPS is used when Direct Mesh is disabled or " +
							"unavailable, but fallback reachability is not guaranteed."
						}
					/>
					<InfoCard
						title="Native Reverse"
						value="Disabled pending rework"
						description={
							"Topology may remain visible in diagnostics, but this path is " +
							"not selectable or active."
						}
					/>
				</div>
			</div>

			<div className="rounded-2xl border border-border/70 bg-muted/20 px-4 py-3">
				<div className="flex flex-wrap items-center justify-between gap-3">
					<div>
						<p className="font-medium">Observer path summary</p>
						<p className="mt-1 text-sm text-muted-foreground">
							Current selected backend observes this node’s remote peers; this
							is not directed preflight coverage or proof of cluster-wide
							health.
						</p>
					</div>
					<Button
						variant="ghost"
						size="sm"
						onClick={onRefresh}
						loading={statusRefreshing}
						iconLeft={<Icon name="tabler:refresh" size={15} />}
					>
						Refresh
					</Button>
				</div>
				<div className="mt-3 grid grid-cols-3 gap-2 text-center text-sm">
					{counts ? (
						<>
							<div className="rounded-xl border border-border/70 bg-card px-2 py-2">
								<div className="font-semibold">{counts.mesh}</div>
								<div className="text-xs text-muted-foreground">Mesh</div>
							</div>
							<div className="rounded-xl border border-border/70 bg-card px-2 py-2">
								<div className="font-semibold">{counts.public}</div>
								<div className="text-xs text-muted-foreground">Public</div>
							</div>
							<div className="rounded-xl border border-border/70 bg-card px-2 py-2">
								<div className="font-semibold">{counts.unknown}</div>
								<div className="text-xs text-muted-foreground">Unknown</div>
							</div>
						</>
					) : (
						<div className="col-span-3 rounded-xl border border-border/70 bg-card px-3 py-2">
							<div className="font-semibold">Unknown</div>
							<div className="text-xs text-muted-foreground">
								Observation unavailable
							</div>
						</div>
					)}
				</div>
				<div className="mt-3 flex flex-wrap items-center gap-2 text-xs">
					<Badge variant={observerDataIncomplete ? "outline" : "secondary"}>
						{observerDataIncomplete ? "Stale or incomplete" : "Current sample"}
					</Badge>
					{observerDataIncomplete ? (
						<span className="text-muted-foreground">
							Some peer observations are stale or unsampled.
						</span>
					) : null}
				</div>
				<p className="mt-2 text-xs text-muted-foreground">
					{!status
						? "No current observation is available."
						: status.peers.length === 0
							? "No remote peer samples are available."
							: `Observed at ${formatTimestamp(status?.generated_at)}.`}
				</p>
			</div>

			<ConfirmDialog
				open={pendingTarget !== null}
				title={
					pendingTarget
						? "Enable Reality Direct Mesh?"
						: "Disable Reality Direct Mesh?"
				}
				description={
					pendingTarget
						? "This changes the cluster-wide control-plane setting and validates " +
							"directed voter paths before applying it. All current voters and " +
							"learners must support the Mesh configuration capability."
						: "This changes the cluster-wide control-plane setting. Existing public " +
							"paths remain available but are not guaranteed reachable. All current " +
							"voters and learners must support the Mesh configuration capability."
				}
				body={
					<div className="space-y-3 text-sm">
						<div className="rounded-xl border border-border/70 bg-muted/20 p-3">
							<p className="font-medium">
								Current observed value:{" "}
								{clusterSettingLabel(currentConfirmationValue)}
							</p>
							<p className="mt-1 text-muted-foreground">
								The confirmation uses a fresh snapshot from{" "}
								{observerBackend || "the current backend"}.
							</p>
						</div>
						{mutationPhase === "refreshing" ? (
							<p className="text-muted-foreground">
								Refreshing the current cluster state…
							</p>
						) : null}
						{mutationPhase === "submitting" ? (
							<p className="text-muted-foreground">
								{pendingTarget
									? "Validating directed cluster paths…"
									: "Applying cluster setting…"}
							</p>
						) : null}
						{targetAlreadyApplied ? (
							<p className="text-info">
								The requested value is already observed. No mutation will be
								sent.
							</p>
						) : null}
					</div>
				}
				footer={
					<AlertDialogFooter>
						<AlertDialogCancel asChild>
							<Button
								type="button"
								variant="ghost"
								disabled={isSubmitting}
								onClick={onCancel}
							>
								Cancel
							</Button>
						</AlertDialogCancel>
						<Button
							type="button"
							onClick={onConfirm}
							loading={mutationPhase === "refreshing" || isSubmitting}
							disabled={
								mutationPhase !== "confirming" ||
								targetAlreadyApplied ||
								statusRefreshing
							}
						>
							{pendingTarget ? "Enable" : "Disable"}
						</Button>
					</AlertDialogFooter>
				}
			/>
			<ConfirmDialog
				open={unlockConfirmOpen}
				title="Unlock the local Mesh safety lock?"
				description={
					"Only continue after independent operations checks confirm that the " +
					"original request has terminated and the cluster has converged. This " +
					"changes only the browser lock; it does not write cluster state."
				}
				onCancel={() => setUnlockConfirmOpen(false)}
				onConfirm={() => {
					setUnlockConfirmOpen(false);
					onUnlock();
				}}
				confirmLabel="I verified termination and convergence"
				cancelLabel="Keep locked"
			/>
		</section>
	);
}
