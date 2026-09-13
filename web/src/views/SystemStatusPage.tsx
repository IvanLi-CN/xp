import { useMutation, useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useMemo, useState } from "react";

import { fetchAdminAlerts } from "@/api/adminAlerts";
import {
	type AdminHistoryRepositoriesResponse,
	fetchAdminHistoryRepositories,
} from "@/api/adminHistoryRepositories";
import {
	type AdminMeshPeer,
	type AdminMeshStatus,
	fetchAdminMeshStatus,
	runAdminMeshProbes,
} from "@/api/adminMesh";
import { fetchAdminNodesRuntime } from "@/api/adminNodeRuntime";
import { Button, IconButton } from "@/components/Button";
import { RepositoryStatusSummary } from "@/components/HistoryRepositoryStatus";
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
	repositoryStatus?: AdminHistoryRepositoriesResponse;
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

type PeerRouteSummary = {
	primary: string;
	secondary?: string;
};

type RendezvousRole = "primary" | "standby";

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
	const health = transport.health === "churning" ? "H2 churning" : "H2";
	return `${health} · ${transport.requests_5m} req · gen ${transport.connection_generation}`;
}

function peerDisplayName(
	peerId: string | null | undefined,
	peers: ReadonlyMap<string, string>,
) {
	if (!peerId) return "Rendezvous";
	return peers.get(peerId) ?? peerId;
}

function rendezvousRolesByPeer(peers: AdminMeshPeer[]) {
	const roles = new Map<string, Set<RendezvousRole>>();
	const add = (nodeId: string | null | undefined, role: RendezvousRole) => {
		if (!nodeId) return;
		const assigned = roles.get(nodeId) ?? new Set<RendezvousRole>();
		assigned.add(role);
		roles.set(nodeId, assigned);
	};

	for (const peer of peers) {
		const route = peer.active_route;
		if (!route) continue;
		add(route.primary_rendezvous, "primary");
		add(route.standby_rendezvous, "standby");
	}

	return roles;
}

function rendezvousRoleLabel(roles: ReadonlySet<RendezvousRole> | undefined) {
	if (!roles?.size) return undefined;
	if (roles.size === 2) return "Rendezvous · primary + standby";
	return `Rendezvous · ${roles.has("primary") ? "primary" : "standby"}`;
}

function routeSummary(
	peer: AdminMeshPeer,
	peers: ReadonlyMap<string, string>,
	rendezvousRoles: ReadonlyMap<string, ReadonlySet<RendezvousRole>>,
): PeerRouteSummary {
	const route = peer.active_route;
	if (route?.kind === "reverse_relay") {
		const activeName = peerDisplayName(route.rendezvous, peers);
		return {
			primary: "Reverse relay",
			secondary: `via ${activeName} · g${route.generation ?? "-"}`,
		};
	}
	if (route?.kind === "reality_direct" || peer.current_path === "mesh") {
		return {
			primary: "Reality direct",
			secondary:
				rendezvousRoleLabel(rendezvousRoles.get(peer.node_id)) ??
				meshTransportLabel(peer),
		};
	}
	if (route?.kind === "public" || peer.current_path === "public") {
		return { primary: "Public fallback", secondary: meshTransportLabel(peer) };
	}
	return { primary: "Awaiting sample", secondary: reasonLabel(peer) };
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
	peerNames,
	rendezvousRoles,
}: {
	peer: AdminMeshPeer;
	onProbe?: () => void;
	probeDisabled?: boolean;
	showMeshTransportReuse?: boolean;
	peerNames: ReadonlyMap<string, string>;
	rendezvousRoles: ReadonlyMap<string, ReadonlySet<RendezvousRole>>;
}) {
	const route = routeSummary(peer, peerNames, rendezvousRoles);
	const secondaryRoute = showMeshTransportReuse
		? route.secondary
		: reasonLabel(peer);
	const content = (
		<>
			<div className="min-w-0" data-peer-cell="identity">
				<div className="flex h-5 min-w-0 items-center gap-2">
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
				<p className="mt-1 truncate whitespace-nowrap font-mono text-xs text-muted-foreground">
					{peer.mesh_url ?? peer.api_base_url}
				</p>
			</div>
			<div className="min-w-0 text-sm" data-peer-cell="route">
				<p
					aria-label={route.primary}
					className="truncate whitespace-nowrap"
					data-peer-route-line
				>
					{route.primary}
				</p>
				{secondaryRoute ? (
					<p
						aria-label={secondaryRoute}
						className={`mt-1 truncate whitespace-nowrap text-xs ${
							peer.mesh_transport?.health === "churning" &&
							showMeshTransportReuse
								? "font-medium text-warning"
								: "text-muted-foreground"
						}`}
						data-peer-route-line
					>
						{secondaryRoute}
					</p>
				) : null}
			</div>
			<div className="min-w-0" data-peer-cell="uptime">
				<MeshUptimeStrip
					buckets={peer.buckets}
					quality={peer.quality}
					label={`${peer.node_name} 24 hour uptime`}
				/>
				<p className="mt-1 truncate whitespace-nowrap text-xs text-muted-foreground">
					24h {percent(peer.availability_24h)} · Mesh{" "}
					{percent(peer.mesh_availability_24h)}
				</p>
			</div>
			<div className="min-w-0 text-sm" data-peer-cell="availability">
				<p className="truncate whitespace-nowrap">
					1h {percent(peer.availability_1h)}
				</p>
				<p className="mt-1 truncate whitespace-nowrap text-xs text-muted-foreground">
					{timestamp(peer.last_transition_at)}
				</p>
			</div>
			<div className="min-w-0 font-mono text-sm" data-peer-cell="latency">
				<p className="truncate whitespace-nowrap">
					{peer.latency_p50_ms ?? "-"} / {peer.latency_p95_ms ?? "-"} ms
				</p>
				<p className="mt-1 truncate whitespace-nowrap text-xs font-sans text-muted-foreground">
					p50 / p95
				</p>
			</div>
			<div className="min-w-0" data-peer-cell="breaker">
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
						"minmax(9rem,1.25fr) minmax(11.5rem,1.6fr) minmax(10rem,1.6fr) " +
						"minmax(5.25rem,.6fr) minmax(5.75rem,.7fr) minmax(4.75rem,.5fr) 4.25rem",
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
	repositoryStatus,
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
		const fallback = status.peers.filter(
			(peer) => peer.current_path === "public",
		).length;
		return { clusterNodes: total + 1, total, fallback };
	}, [status.peers]);
	const canaryStatus = !status.local.canary.enabled
		? "disabled"
		: status.local.canary.last_error
			? "degraded"
			: "ready";
	const peerNames = useMemo(
		() => new Map(status.peers.map((peer) => [peer.node_id, peer.node_name])),
		[status.peers],
	);
	const rendezvousRoles = useMemo(
		() => rendezvousRolesByPeer(status.peers),
		[status.peers],
	);

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
						label="Cluster nodes"
						value={String(summary.clusterNodes)}
						detail={`1 local · ${summary.total} remote`}
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
				<h2 className="mb-3 text-lg font-semibold">Peer transport</h2>
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
								peerNames={peerNames}
								rendezvousRoles={rendezvousRoles}
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

			{repositoryStatus ? (
				<RepositoryStatusSummary status={repositoryStatus} compact />
			) : null}
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
	const historyRepositoriesCapability = useApiCapability(
		"admin.history-repositories",
	);
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
	const repositoriesQuery = useQuery({
		queryKey: ["adminHistoryRepositories", adminToken],
		enabled:
			adminToken.length > 0 &&
			(historyRepositoriesCapability.available || !runtime.isOnline),
		queryFn: ({ signal }) => fetchAdminHistoryRepositories(adminToken, signal),
	});
	const repositoriesState = useQueryWithOfflineFallback(
		["adminHistoryRepositories", adminToken],
		repositoriesQuery,
	);
	const probe = useMutation({
		mutationFn: (nodeIds: string[]) => runAdminMeshProbes(adminToken, nodeIds),
		onSuccess: () => meshQuery.refetch(),
	});
	const meshData = meshCapability.available ? meshState.data : undefined;
	const latestAt = latestQueryDataUpdatedAt([
		meshCapability.available ? meshState : null,
		runtimeQuery,
		alertsQuery,
		historyRepositoriesCapability.available ? repositoriesState : null,
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
			{latestAt !== null &&
			(!runtime.isOnline || meshState.isError || repositoriesState.isError) ? (
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
				repositoryStatus={repositoriesState.data}
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
