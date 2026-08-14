import { useMutation, useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useMemo, useState } from "react";

import { fetchAdminAlerts } from "@/api/adminAlerts";
import {
	type AdminMeshPeer,
	type AdminMeshStatus,
	fetchAdminMeshStatus,
	runAdminMeshProbes,
} from "@/api/adminMesh";
import { fetchAdminNodesRuntime } from "@/api/adminNodeRuntime";
import { Button, IconButton } from "@/components/Button";
import { Icon } from "@/components/Icon";
import { MeshUptimeStrip } from "@/components/MeshUptimeStrip";
import { PageHeader } from "@/components/PageHeader";
import { CapabilityUnavailableState, PageState } from "@/components/PageState";
import { ReadStateBanner } from "@/components/ReadStateBanner";
import { readAdminToken } from "@/components/auth";
import { Badge } from "@/components/ui/badge";
import { useAppRuntime } from "@/offline/appRuntime";
import {
	formatSyncTimestamp,
	hasQueryData,
	latestQueryDataUpdatedAt,
	queryIsOfflineBlocked,
} from "@/offline/queryReadState";
import { useQueryWithOfflineFallback } from "@/offline/useQueryWithOfflineFallback";
import { useApiCapability } from "../api/useApiCompatibility";

export type SystemStatusSurfaceProps = {
	status: AdminMeshStatus;
	components?: Array<{ component: string; status: string }>;
	isRefreshing?: boolean;
	isProbing?: boolean;
	readOnly?: boolean;
	showMeshTransportReuse?: boolean;
	onRefresh?: () => void;
	onProbeAll?: () => void;
	onProbePeer?: (nodeId: string) => void;
};

const qualityLabel = {
	good: "Good",
	slow: "Slow",
	unstable: "Unstable",
	down: "Down",
	unknown: "Unknown",
} as const;

const qualityVariant = {
	good: "success",
	slow: "warning",
	unstable: "warning",
	down: "destructive",
	unknown: "outline",
} as const;

function percent(value: number | null) {
	return value === null ? "-" : `${(value * 100).toFixed(1)}%`;
}

function timestamp(value: string | null) {
	if (!value) return "No sample";
	const parsed = Date.parse(value);
	return Number.isFinite(parsed)
		? new Intl.DateTimeFormat(undefined, {
				hour: "2-digit",
				minute: "2-digit",
				month: "short",
				day: "numeric",
			}).format(parsed)
		: value;
}

function routeLabel(peer: AdminMeshPeer) {
	if (!peer.current_path) return "Awaiting sample";
	return peer.current_path === "mesh" ? "Reality Mesh" : "Public fallback";
}

const meshReasonLabel: Record<
	NonNullable<AdminMeshPeer["mesh_reason"]>,
	string
> = {
	mesh_available: "Mesh available",
	missing_endpoint: "Mesh endpoint missing",
	ambiguous_endpoint: "Multiple Mesh endpoints",
	invalid_access_host: "Invalid access host",
	no_sample: "Awaiting Mesh sample",
	transport_timeout: "Mesh timed out",
	transport_error: "Mesh transport error",
	protocol_rejected: "Mesh protocol rejected",
	fallback_active: "Using public fallback",
};

function reasonLabel(peer: AdminMeshPeer) {
	return peer.mesh_reason
		? meshReasonLabel[peer.mesh_reason]
		: "Mesh reason unknown";
}

function meshTransportLabel(peer: AdminMeshPeer) {
	const transport = peer.mesh_transport;
	if (
		!transport ||
		transport.protocol !== "h2" ||
		transport.connection_generation === 0
	) {
		return "Reuse data unavailable";
	}
	const prefix = transport.health === "churning" ? "Churning · " : "";
	return (
		`${prefix}H2 · ${transport.requests_5m} req / ` +
		`${transport.connection_starts_5m} starts · gen ${transport.connection_generation}`
	);
}

function meshTransportTitle(peer: AdminMeshPeer) {
	const transport = peer.mesh_transport;
	return transport
		? `1h: ${transport.requests_1h} requests / ${transport.connection_starts_1h} starts`
		: undefined;
}

function BreakerBadge({ state }: { state: AdminMeshPeer["breaker"] }) {
	const label =
		state === "half_open"
			? "Half-open"
			: state[0].toUpperCase() + state.slice(1);
	return (
		<Badge
			variant={
				state === "open"
					? "destructive"
					: state === "disabled"
						? "outline"
						: "secondary"
			}
			size="sm"
		>
			{label}
		</Badge>
	);
}

function PeerRows({
	peer,
	onProbe,
	probeDisabled,
	showMeshTransportReuse,
}: {
	peer: AdminMeshPeer;
	onProbe?: () => void;
	probeDisabled?: boolean;
	showMeshTransportReuse?: boolean;
}) {
	const showTransport =
		showMeshTransportReuse && peer.mesh_capability === "enabled";
	const content = (
		<>
			<div className="min-w-0">
				<div className="flex items-center gap-2">
					<span className="truncate font-medium">{peer.node_name}</span>
					<Badge variant={qualityVariant[peer.quality]} size="sm">
						{qualityLabel[peer.quality]}
					</Badge>
					{peer.stale ? (
						<Badge variant="outline" size="sm">
							Stale
						</Badge>
					) : null}
				</div>
				<p className="mt-1 truncate font-mono text-xs text-muted-foreground">
					{peer.mesh_url ?? peer.api_base_url}
				</p>
			</div>
			<div className="min-w-0 text-sm">
				<p>{routeLabel(peer)}</p>
				<p className="mt-1 truncate text-xs text-muted-foreground">
					{reasonLabel(peer)}
				</p>
				{showTransport ? (
					<p
						data-mesh-transport={peer.mesh_transport?.health ?? "unknown"}
						className={`mt-1 break-words text-xs leading-5 ${
							peer.mesh_transport?.health === "churning"
								? "font-medium text-warning"
								: "text-muted-foreground"
						}`}
						title={meshTransportTitle(peer)}
					>
						{meshTransportLabel(peer)}
					</p>
				) : null}
				<p className="mt-1 text-xs text-muted-foreground">
					{timestamp(peer.last_transition_at)}
				</p>
			</div>
			<div className="min-w-0">
				<MeshUptimeStrip
					buckets={peer.buckets}
					quality={peer.quality}
					label={`${peer.node_name} 24 hour uptime`}
				/>
				<div className="mt-1 flex justify-between text-xs text-muted-foreground">
					<span>24h uptime {percent(peer.availability_24h)}</span>
					<span>Mesh {percent(peer.mesh_availability_24h)}</span>
				</div>
			</div>
			<div className="min-w-0 text-sm">
				<p>{percent(peer.availability_1h)}</p>
				<p className="mt-1 text-xs text-muted-foreground">1h availability</p>
			</div>
			<div className="min-w-0 font-mono text-sm">
				<p>
					{peer.latency_p50_ms ?? "-"} / {peer.latency_p95_ms ?? "-"}
				</p>
				<p className="mt-1 text-xs font-sans text-muted-foreground">
					p50 / p95 ms
				</p>
			</div>
			<div className="min-w-0">
				<BreakerBadge state={peer.breaker} />
			</div>
		</>
	);
	return (
		<>
			<div
				data-peer-row={peer.node_id}
				className="hidden items-center gap-3 border-b border-border/70 py-3 xl:grid"
				style={{
					gridTemplateColumns:
						"minmax(9rem,1.35fr) minmax(7rem,.9fr) minmax(12rem,2fr) " +
						"minmax(6rem,.65fr) minmax(6rem,.75fr) minmax(5.5rem,.6fr) 4.25rem",
				}}
			>
				{content}
				<div className="flex w-[4.25rem] items-center justify-end gap-1">
					<IconButton
						label={`Probe ${peer.node_name}`}
						tooltip={`Probe ${peer.node_name}`}
						variant="ghost"
						onClick={onProbe}
						disabled={probeDisabled || !onProbe}
					>
						<Icon name="tabler:activity" size={16} />
					</IconButton>
					<IconButton
						label={`Open ${peer.node_name} details`}
						tooltip={`Open ${peer.node_name} details`}
						asChild
						variant="ghost"
					>
						<Link to="/nodes/$nodeId" params={{ nodeId: peer.node_id }}>
							<Icon name="tabler:arrow-up-right" size={16} />
						</Link>
					</IconButton>
				</div>
			</div>
			<div className="space-y-3 border-b border-border/70 py-4 xl:hidden">
				{content}
				<div className="flex justify-end gap-2">
					<Button
						variant="secondary"
						size="sm"
						onClick={onProbe}
						disabled={probeDisabled || !onProbe}
						iconLeft={<Icon name="tabler:activity" size={15} />}
					>
						Probe
					</Button>
					<Button asChild variant="ghost" size="sm">
						<Link to="/nodes/$nodeId" params={{ nodeId: peer.node_id }}>
							Details
						</Link>
					</Button>
				</div>
			</div>
		</>
	);
}

export function SystemStatusSurface({
	status,
	components = [],
	isRefreshing,
	isProbing,
	readOnly,
	showMeshTransportReuse,
	onRefresh,
	onProbeAll,
	onProbePeer,
}: SystemStatusSurfaceProps) {
	const summary = useMemo(() => {
		const total = status.peers.length;
		const meshReady = status.peers.filter(
			(peer) => peer.current_path === "mesh",
		).length;
		const fallback = status.peers.filter(
			(peer) => peer.current_path === "public",
		).length;
		return { total, meshReady, fallback };
	}, [status.peers]);
	const canaryStatus = !status.local.canary.enabled
		? "disabled"
		: status.local.canary.last_error
			? "degraded"
			: "ready";

	return (
		<div className="space-y-7">
			<PageHeader
				title="System status"
				description="Control-plane reachability, transport quality, and node runtime health."
				meta={
					<>
						<Badge variant="secondary" size="sm">
							{status.local.role}
						</Badge>
						<span className="font-mono text-xs text-muted-foreground">
							term {status.local.term} · rev {status.revision}
						</span>
					</>
				}
				actions={
					<>
						<Button
							className="w-28"
							variant="secondary"
							size="sm"
							loading={isRefreshing}
							onClick={onRefresh}
							iconLeft={<Icon name="tabler:refresh" size={15} />}
						>
							Refresh
						</Button>
						<Button
							className="w-28"
							size="sm"
							loading={isProbing}
							disabled={readOnly || status.peers.length === 0}
							onClick={onProbeAll}
							iconLeft={<Icon name="tabler:activity-heartbeat" size={15} />}
						>
							Probe all
						</Button>
					</>
				}
			/>

			<section className="border-y border-border/70 py-4">
				<div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-5">
					<StatusFact
						label="Local node"
						value={status.local.node_name}
						detail={status.local.node_id}
					/>
					<StatusFact
						label="Raft"
						value={status.local.role}
						detail={
							status.local.leader_api_base_url || `term ${status.local.term}`
						}
					/>
					<StatusFact
						label="Mesh peers"
						value={`${summary.meshReady}/${summary.total}`}
						detail="on Reality Mesh"
					/>
					<StatusFact
						label="Public fallback"
						value={String(summary.fallback)}
						detail="active paths"
					/>
					<StatusFact
						label="Last snapshot"
						value={timestamp(status.generated_at)}
						detail={status.local.cluster_id}
					/>
				</div>
			</section>

			<section>
				<div className="mb-3 flex items-end justify-between gap-4">
					<div>
						<h2 className="text-lg font-semibold">Peer transport</h2>
						<p className="mt-1 text-sm text-muted-foreground">
							A blue edge on a timeline marks end-to-end success through public
							fallback.
						</p>
					</div>
					<span className="text-xs text-muted-foreground">No pagination</span>
				</div>
				{status.peers.length === 0 ? (
					<div
						className={
							"border border-dashed border-border/80 px-4 py-10 text-center " +
							"text-sm text-muted-foreground"
						}
					>
						No remote members have been recorded for this cluster.
					</div>
				) : (
					<div className="border-t border-border/70">
						{status.peers.map((peer) => (
							<PeerRows
								key={peer.node_id}
								peer={peer}
								showMeshTransportReuse={showMeshTransportReuse}
								probeDisabled={readOnly}
								onProbe={
									readOnly ? undefined : () => onProbePeer?.(peer.node_id)
								}
							/>
						))}
					</div>
				)}
			</section>

			<section className="grid gap-6 border-t border-border/70 pt-6 lg:grid-cols-[1.2fr_0.8fr]">
				<div>
					<h2 className="text-lg font-semibold">Runtime components</h2>
					<div className="mt-3 grid grid-cols-2 border-t border-border/70 sm:grid-cols-5">
						{[...components, { component: "canary", status: canaryStatus }].map(
							(item) => (
								<div
									key={item.component}
									className="border-b border-r border-border/70 px-3 py-3"
								>
									<p className="text-xs uppercase text-muted-foreground">
										{item.component}
									</p>
									<p className="mt-1 font-medium capitalize">{item.status}</p>
								</div>
							),
						)}
					</div>
				</div>
				<div>
					<h2 className="text-lg font-semibold">Recent mesh events</h2>
					<ol className="mt-3 divide-y divide-border/70 border-t border-border/70">
						{status.events
							.slice(-6)
							.reverse()
							.map((event) => (
								<li
									key={`${event.at}-${event.peer_id}-${event.kind}`}
									className="py-2 text-sm"
								>
									<div className="flex justify-between gap-3">
										<span className="font-medium">
											{event.kind.replaceAll("_", " ")}
										</span>
										<span className="shrink-0 text-xs text-muted-foreground">
											{timestamp(event.at)}
										</span>
									</div>
									<p className="mt-1 text-muted-foreground">{event.message}</p>
								</li>
							))}
						{status.events.length === 0 ? (
							<li className="py-4 text-sm text-muted-foreground">
								No transport events yet.
							</li>
						) : null}
					</ol>
				</div>
			</section>
		</div>
	);
}

function StatusFact({
	label,
	value,
	detail,
}: { label: string; value: string; detail: string }) {
	return (
		<div className="min-w-0">
			<p className="text-xs uppercase text-muted-foreground">{label}</p>
			<p className="mt-1 truncate text-base font-semibold">{value}</p>
			<p className="mt-1 truncate text-xs text-muted-foreground">{detail}</p>
		</div>
	);
}

export function SystemStatusPage() {
	const runtime = useAppRuntime();
	const [adminToken] = useState(() => readAdminToken());
	const meshCapability = useApiCapability("admin.mesh");
	const meshTransportCapability = useApiCapability(
		"admin.mesh-transport-reuse",
	);
	const nodesCapability = useApiCapability("admin.nodes");
	const alertsCapability = useApiCapability("admin.alerts");
	const meshQuery = useQuery({
		queryKey: ["adminMeshStatus", adminToken],
		enabled: adminToken.length > 0 && meshCapability.available,
		queryFn: ({ signal }) => fetchAdminMeshStatus(adminToken, signal),
		refetchInterval: 30_000,
	});
	const meshState = useQueryWithOfflineFallback(
		["adminMeshStatus", adminToken],
		meshQuery,
	);
	const runtimeQuery = useQuery({
		queryKey: ["adminNodesRuntime", adminToken],
		enabled: adminToken.length > 0 && nodesCapability.available,
		queryFn: ({ signal }) => fetchAdminNodesRuntime(adminToken, signal),
	});
	const alertsQuery = useQuery({
		queryKey: ["adminAlerts", adminToken],
		enabled: adminToken.length > 0 && alertsCapability.available,
		queryFn: ({ signal }) => fetchAdminAlerts(adminToken, signal),
	});
	const probe = useMutation({
		mutationFn: (nodeIds: string[]) => runAdminMeshProbes(adminToken, nodeIds),
		onSuccess: () => meshQuery.refetch(),
	});
	const meshData = meshCapability.available ? meshState.data : undefined;
	const latestAt = latestQueryDataUpdatedAt([
		meshCapability.available ? meshState : null,
		runtimeQuery,
		alertsQuery,
	]);
	const localComponents =
		runtimeQuery.data?.items
			.find((item) => item.node_id === meshData?.local.node_id)
			?.components.map((item) => ({
				component: item.component,
				status: item.status,
			})) ?? [];

	if (adminToken.length === 0)
		return (
			<PageState
				variant="empty"
				title="Admin token required"
				description="Set an admin token to inspect mesh status."
			/>
		);
	if (
		meshCapability.available &&
		meshState.isLoading &&
		!hasQueryData(meshState)
	)
		return (
			<PageState
				variant="loading"
				title="Loading system status"
				description="Collecting the local telemetry snapshot."
			/>
		);
	if (
		meshCapability.available &&
		!hasQueryData(meshState) &&
		queryIsOfflineBlocked(meshState, runtime.isOnline)
	)
		return (
			<PageState
				variant="offline"
				title="Offline snapshot unavailable"
				description="Open System status once while online to keep a local snapshot."
			/>
		);
	if (meshCapability.available && meshState.isError && !hasQueryData(meshState))
		return (
			<PageState
				variant="error"
				title="Failed to load system status"
				description="The mesh telemetry endpoint did not return a usable snapshot."
				error={meshState.error}
				action={
					<Button variant="secondary" onClick={() => meshState.refetch()}>
						Retry
					</Button>
				}
			/>
		);
	if (!meshData) {
		return (
			<div className="space-y-5">
				<PageHeader
					title="System status"
					description="Control-plane reachability, transport quality, and node runtime health."
				/>
				<section className="border-t border-border/70 pt-5">
					<CapabilityUnavailableState
						title="Mesh status unavailable"
						reason={meshCapability.reason}
					/>
				</section>
			</div>
		);
	}

	return (
		<div className="space-y-5">
			{latestAt !== null && (!runtime.isOnline || meshState.isError) ? (
				<ReadStateBanner
					tone={!runtime.isOnline ? "warning" : "info"}
					variant="inline"
					title={
						!runtime.isOnline
							? "Offline read-only snapshot"
							: "Showing cached system status"
					}
					description={`Last successful sync: ${formatSyncTimestamp(latestAt)}.`}
				/>
			) : null}
			<SystemStatusSurface
				status={meshData}
				components={localComponents}
				isRefreshing={meshState.isFetching}
				isProbing={probe.isPending}
				readOnly={runtime.isReadOnly}
				showMeshTransportReuse={meshTransportCapability.available}
				onRefresh={() => meshState.refetch()}
				onProbeAll={
					meshCapability.available ? () => probe.mutate([]) : undefined
				}
				onProbePeer={
					meshCapability.available
						? (nodeId) => probe.mutate([nodeId])
						: undefined
				}
			/>
		</div>
	);
}
