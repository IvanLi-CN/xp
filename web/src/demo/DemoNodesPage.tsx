import {
	Link,
	useLocation,
	useNavigate,
	useParams,
} from "@tanstack/react-router";
import { useMemo, useState } from "react";

import { Badge } from "@/components/ui/badge";

import { Button } from "../components/Button";
import {
	ModuleTabsLayout,
	ModuleTabsPanel,
} from "../components/ModuleTabsLayout";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
import { Input } from "../components/ui/input";
import { formatGb, nodeStatusVariant, shortDate } from "./format";
import { useDemo } from "./store";

type DemoNodesTab = "nodes" | "join" | "repositories";
type DemoNodesTabPath =
	| "/demo/nodes"
	| "/demo/nodes/join"
	| "/demo/nodes/repositories";

const DEMO_NODES_TABS = [
	{ value: "nodes", label: "Nodes" },
	{ value: "join", label: "Join node" },
	{ value: "repositories", label: "History repositories" },
] satisfies Array<{ value: DemoNodesTab; label: string }>;

const DEMO_NODES_TAB_PATHS: Record<DemoNodesTab, DemoNodesTabPath> = {
	nodes: "/demo/nodes",
	join: "/demo/nodes/join",
	repositories: "/demo/nodes/repositories",
};

function demoNodesTabFromPath(pathname: string): DemoNodesTab {
	if (pathname === DEMO_NODES_TAB_PATHS.join) return "join";
	if (pathname === DEMO_NODES_TAB_PATHS.repositories) return "repositories";
	return "nodes";
}

export function DemoNodesPage() {
	const location = useLocation();
	const navigate = useNavigate();
	const { state } = useDemo();
	const [query, setQuery] = useState("");
	const [status, setStatus] = useState("all");
	const activeTab = demoNodesTabFromPath(location.pathname);
	const nodes = useMemo(() => {
		const q = query.trim().toLowerCase();
		return state.nodes.filter((node) => {
			const matchesQuery =
				q.length === 0 ||
				node.name.toLowerCase().includes(q) ||
				node.accessHost.toLowerCase().includes(q) ||
				node.region.toLowerCase().includes(q);
			const matchesStatus = status === "all" || node.status === status;
			return matchesQuery && matchesStatus;
		});
	}, [query, state.nodes, status]);
	const handleTabChange = (value: string) => {
		if (!(value in DEMO_NODES_TAB_PATHS)) return;
		void navigate({ to: DEMO_NODES_TAB_PATHS[value as DemoNodesTab] });
	};

	return (
		<div className="space-y-6">
			<PageHeader
				title="Nodes"
				description="Inspect runtime state, role, quota pressure, and access hosts."
			/>

			<ModuleTabsLayout
				options={DEMO_NODES_TABS}
				value={activeTab}
				onValueChange={handleTabChange}
				ariaLabel="Nodes sections"
				mobileAriaLabel="Nodes section"
			>
				<ModuleTabsPanel value="nodes" keepMounted>
					<NodesInventoryPanel
						nodes={nodes}
						query={query}
						status={status}
						onQueryChange={setQuery}
						onStatusChange={setStatus}
					/>
				</ModuleTabsPanel>
				<ModuleTabsPanel value="join" keepMounted>
					<DemoJoinNodePanel
						canCreateToken={state.session?.role !== "viewer"}
					/>
				</ModuleTabsPanel>
				<ModuleTabsPanel value="repositories" keepMounted>
					<DemoHistoryRepositoriesPanel nodes={state.nodes} />
				</ModuleTabsPanel>
			</ModuleTabsLayout>
		</div>
	);
}

function NodesInventoryPanel(props: {
	nodes: ReturnType<typeof useDemo>["state"]["nodes"];
	query: string;
	status: string;
	onQueryChange: (value: string) => void;
	onStatusChange: (value: string) => void;
}) {
	return (
		<div className="space-y-4">
			<div className="grid gap-3 md:grid-cols-[minmax(0,1fr)_12rem]">
				<Input
					value={props.query}
					onChange={(event) => props.onQueryChange(event.target.value)}
					placeholder="Search node, host, or region"
					aria-label="Search nodes"
				/>
				<select
					className="xp-select"
					value={props.status}
					aria-label="Filter node status"
					onChange={(event) => props.onStatusChange(event.target.value)}
				>
					<option value="all">All statuses</option>
					<option value="healthy">Healthy</option>
					<option value="degraded">Degraded</option>
					<option value="offline">Offline</option>
				</select>
			</div>

			{props.nodes.length === 0 ? (
				<PageState
					variant="empty"
					title="No matching nodes"
					description="Clear the search or switch the scenario seed."
					action={
						<Button variant="secondary" onClick={() => props.onQueryChange("")}>
							Clear search
						</Button>
					}
				/>
			) : (
				<div className="xp-table-wrap">
					<table className="xp-table xp-table-zebra">
						<thead>
							<tr>
								<th>Node</th>
								<th>Region</th>
								<th>Status</th>
								<th>Role</th>
								<th>Quota</th>
								<th>Last seen</th>
							</tr>
						</thead>
						<tbody>
							{props.nodes.map((node) => (
								<tr key={node.id}>
									<td>
										<Link
											className="font-medium hover:underline"
											to="/demo/nodes/$nodeId"
											params={{ nodeId: node.id }}
										>
											{node.name}
										</Link>
										<p className="font-mono text-xs text-muted-foreground">
											{node.accessHost}
										</p>
									</td>
									<td>{node.region}</td>
									<td>
										<Badge variant={nodeStatusVariant(node.status)} size="sm">
											{node.status}
										</Badge>
									</td>
									<td className="font-mono text-xs">{node.role}</td>
									<td className="font-mono text-xs">
										{formatGb(node.quotaUsedGb)} / {formatGb(node.quotaLimitGb)}
									</td>
									<td className="font-mono text-xs">
										{shortDate(node.lastSeenAt)}
									</td>
								</tr>
							))}
						</tbody>
					</table>
				</div>
			)}
		</div>
	);
}

function DemoJoinNodePanel(props: { canCreateToken: boolean }) {
	const [ttlSeconds, setTtlSeconds] = useState("3600");
	const [joinToken, setJoinToken] = useState<string | null>(null);

	return (
		<section className="space-y-4">
			<div>
				<h2 className="text-lg font-semibold">Create a join token</h2>
				<p className="text-sm text-muted-foreground">
					Generate a token and share it with the node you want to join.
				</p>
			</div>
			<div className="grid gap-4 md:grid-cols-[minmax(0,1fr)_auto] md:items-end">
				<div className="xp-field-stack">
					<label className="text-sm font-medium" htmlFor="demo-join-ttl">
						TTL (seconds)
					</label>
					<Input
						id="demo-join-ttl"
						type="number"
						min={60}
						step={60}
						value={ttlSeconds}
						onChange={(event) => setTtlSeconds(event.target.value)}
					/>
				</div>
				<Button
					variant="secondary"
					disabled={!props.canCreateToken || Number(ttlSeconds) <= 0}
					onClick={() => setJoinToken(`demo-join-${ttlSeconds}`)}
				>
					Create token
				</Button>
			</div>
			{joinToken ? (
				<div className="space-y-2 border-t border-border/70 pt-4">
					<p className="text-xs uppercase tracking-wide text-muted-foreground">
						Join token
					</p>
					<p className="break-all font-mono text-sm">{joinToken}</p>
				</div>
			) : null}
		</section>
	);
}

function DemoHistoryRepositoriesPanel(props: {
	nodes: ReturnType<typeof useDemo>["state"]["nodes"];
}) {
	return (
		<section className="space-y-4">
			<div className="flex flex-wrap items-center justify-between gap-3">
				<p className="text-sm text-muted-foreground">Replica status</p>
				<Badge variant="success">reachable</Badge>
			</div>
			<div className="divide-y divide-border/70 border-y border-border/70">
				{props.nodes.map((node) => (
					<div
						key={node.id}
						className="grid gap-3 py-4 md:grid-cols-[minmax(0,1fr)_auto] md:items-center"
					>
						<div>
							<p className="font-medium">{node.name}</p>
							<p className="font-mono text-xs text-muted-foreground">
								{node.id}
							</p>
							<p className="mt-2 text-sm text-muted-foreground">
								{node.status} · {formatGb(node.quotaUsedGb)} used
							</p>
						</div>
						<div className="flex flex-wrap items-center gap-2">
							<Badge
								variant={node.status === "healthy" ? "success" : "warning"}
							>
								{node.status === "healthy" ? "converged" : "degraded"}
							</Badge>
							<span className="font-mono text-xs text-muted-foreground">
								{node.quotaLimitGb === null
									? "unlimited quota"
									: `${formatGb(node.quotaLimitGb)} quota`}
							</span>
						</div>
					</div>
				))}
			</div>
		</section>
	);
}

export function DemoNodeDetailsPage() {
	const { nodeId } = useParams({ from: "/demo/nodes/$nodeId" });
	const { state } = useDemo();
	const node = state.nodes.find((item) => item.id === nodeId);
	const canWrite = state.session?.role !== "viewer";
	const endpoints = state.endpoints.filter(
		(endpoint) => endpoint.nodeId === nodeId,
	);

	if (!node) {
		return (
			<PageState
				variant="error"
				title="Node not found"
				description="The selected demo node does not exist in this seed."
				action={
					<Button asChild>
						<Link to="/demo/nodes">Back to nodes</Link>
					</Button>
				}
			/>
		);
	}

	return (
		<div className="space-y-6">
			<PageHeader
				title={node.name}
				description={node.accessHost}
				meta={
					<>
						<Badge variant={nodeStatusVariant(node.status)}>
							{node.status}
						</Badge>
						<Badge variant="ghost" className="font-mono">
							term {node.term}
						</Badge>
					</>
				}
				actions={
					<Button asChild variant="ghost" size="sm">
						<Link to="/demo/nodes">Back</Link>
					</Button>
				}
			/>

			<div className="grid gap-4 md:grid-cols-3">
				<div className="xp-panel-muted p-4">
					<p className="text-xs uppercase tracking-[0.18em] text-muted-foreground">
						Role
					</p>
					<p className="mt-2 font-mono text-lg">{node.role}</p>
				</div>
				<div className="xp-panel-muted p-4">
					<p className="text-xs uppercase tracking-[0.18em] text-muted-foreground">
						Latency
					</p>
					<p className="mt-2 font-mono text-lg">
						{node.latencyMs === null ? "unreachable" : `${node.latencyMs} ms`}
					</p>
				</div>
				<div className="xp-panel-muted p-4">
					<p className="text-xs uppercase tracking-[0.18em] text-muted-foreground">
						Quota
					</p>
					<p className="mt-2 font-mono text-lg">
						{formatGb(node.quotaUsedGb)} / {formatGb(node.quotaLimitGb)}
					</p>
				</div>
			</div>

			<section className="xp-card">
				<div className="xp-card-body">
					<h2 className="xp-card-title">Endpoints on this node</h2>
					{endpoints.length === 0 ? (
						<PageState
							variant="empty"
							title="No endpoints on this node"
							description="Create one from the endpoints flow."
							action={
								canWrite ? (
									<Button asChild>
										<Link to="/demo/endpoints/new">New endpoint</Link>
									</Button>
								) : (
									<Button disabled>New endpoint</Button>
								)
							}
						/>
					) : (
						<div className="xp-table-wrap">
							<table className="xp-table">
								<thead>
									<tr>
										<th>Endpoint</th>
										<th>Port</th>
										<th>Users</th>
										<th>Probe</th>
									</tr>
								</thead>
								<tbody>
									{endpoints.map((endpoint) => (
										<tr key={endpoint.id}>
											<td>
												<Link
													className="font-medium hover:underline"
													to="/demo/endpoints/$endpointId"
													params={{ endpointId: endpoint.id }}
												>
													{endpoint.name}
												</Link>
											</td>
											<td className="font-mono text-xs">{endpoint.port}</td>
											<td className="font-mono text-xs">
												{endpoint.assignedUserIds.length}
											</td>
											<td className="font-mono text-xs">
												{endpoint.probeLatencyMs === null
													? "not run"
													: `${endpoint.probeLatencyMs} ms`}
											</td>
										</tr>
									))}
								</tbody>
							</table>
						</div>
					)}
				</div>
			</section>
		</div>
	);
}
