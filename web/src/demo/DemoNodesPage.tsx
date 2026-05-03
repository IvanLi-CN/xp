import { Link, useParams } from "@tanstack/react-router";
import { useMemo, useState } from "react";

import { Badge } from "@/components/ui/badge";

import { Button } from "../components/Button";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
import { Input } from "../components/ui/input";
import { formatGb, nodeStatusVariant, shortDate } from "./format";
import { useDemo } from "./store";

export function DemoNodesPage() {
	const { state } = useDemo();
	const [query, setQuery] = useState("");
	const [status, setStatus] = useState("all");
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

	return (
		<div className="space-y-6">
			<PageHeader
				title="Nodes"
				description="Inspect runtime state, role, quota pressure, and access hosts."
			/>

			<div className="grid gap-3 md:grid-cols-[minmax(0,1fr)_12rem]">
				<Input
					value={query}
					onChange={(event) => setQuery(event.target.value)}
					placeholder="Search node, host, or region"
					aria-label="Search nodes"
				/>
				<select
					className="xp-select"
					value={status}
					aria-label="Filter node status"
					onChange={(event) => setStatus(event.target.value)}
				>
					<option value="all">All statuses</option>
					<option value="healthy">Healthy</option>
					<option value="degraded">Degraded</option>
					<option value="offline">Offline</option>
				</select>
			</div>

			{nodes.length === 0 ? (
				<PageState
					variant="empty"
					title="No matching nodes"
					description="Clear the search or switch the scenario seed."
					action={
						<Button variant="secondary" onClick={() => setQuery("")}>
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
							{nodes.map((node) => (
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
