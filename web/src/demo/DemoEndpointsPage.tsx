import { Link, useNavigate, useParams } from "@tanstack/react-router";
import { useEffect, useMemo, useState } from "react";

import { Badge } from "@/components/ui/badge";

import { Button } from "../components/Button";
import { CopyButton } from "../components/CopyButton";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
import { useToast } from "../components/Toast";
import { buttonVariants } from "../components/ui/button";
import { Input } from "../components/ui/input";
import {
	endpointKindLabel,
	endpointStatusVariant,
	shortDate,
	subscriptionUrl,
} from "./format";
import { useDemo } from "./store";
import type { DemoEndpoint } from "./types";

export function DemoEndpointsPage() {
	const navigate = useNavigate();
	const { state, createProbeRun } = useDemo();
	const [query, setQuery] = useState("");
	const [status, setStatus] = useState("all");
	const [sort, setSort] = useState("name");
	const [page, setPage] = useState(1);
	const [runningProbe, setRunningProbe] = useState(false);
	const pageSize = 5;
	const canWrite = state.session?.role !== "viewer";

	const filtered = useMemo(() => {
		const q = query.trim().toLowerCase();
		const items = state.endpoints.filter((endpoint) => {
			const node = state.nodes.find(
				(candidate) => candidate.id === endpoint.nodeId,
			);
			const matchesQuery =
				q.length === 0 ||
				endpoint.name.toLowerCase().includes(q) ||
				endpoint.id.toLowerCase().includes(q) ||
				node?.name.toLowerCase().includes(q);
			const matchesStatus = status === "all" || endpoint.status === status;
			return matchesQuery && matchesStatus;
		});
		items.sort((a, b) => {
			if (sort === "users") {
				return b.assignedUserIds.length - a.assignedUserIds.length;
			}
			if (sort === "probe") {
				return (a.probeLatencyMs ?? 9999) - (b.probeLatencyMs ?? 9999);
			}
			return a.name.localeCompare(b.name);
		});
		return items;
	}, [query, sort, state.endpoints, state.nodes, status]);

	const pages = Math.max(1, Math.ceil(filtered.length / pageSize));
	const safePage = Math.min(page, pages);
	useEffect(() => {
		if (page > pages) setPage(pages);
	}, [page, pages]);
	const visible = filtered.slice(
		(safePage - 1) * pageSize,
		safePage * pageSize,
	);

	return (
		<div className="space-y-6">
			<PageHeader
				title="Endpoints"
				description="Create, filter, probe, and inspect ingress endpoints."
				actions={
					<>
						<Button
							variant="secondary"
							loading={runningProbe}
							disabled={
								!canWrite || runningProbe || state.endpoints.length === 0
							}
							onClick={() => {
								const endpoint = filtered[0] ?? state.endpoints[0];
								if (!endpoint) return;
								setRunningProbe(true);
								window.setTimeout(() => {
									const run = createProbeRun(endpoint.id);
									setRunningProbe(false);
									navigate({
										to: "/demo/endpoints/probe/runs/$runId",
										params: { runId: run.id },
									});
								}, 500);
							}}
						>
							Run probe
						</Button>
						{canWrite ? (
							<Button asChild>
								<Link to="/demo/endpoints/new">New endpoint</Link>
							</Button>
						) : (
							<Button disabled>New endpoint</Button>
						)}
						{!canWrite ? (
							<Badge variant="warning">viewer role is read-only</Badge>
						) : null}
					</>
				}
			/>

			<div className="grid gap-3 lg:grid-cols-[minmax(0,1fr)_10rem_10rem]">
				<Input
					value={query}
					onChange={(event) => {
						setQuery(event.target.value);
						setPage(1);
					}}
					placeholder="Search endpoint, ID, or node"
					aria-label="Search endpoints"
				/>
				<select
					className="xp-select"
					value={status}
					aria-label="Filter endpoint status"
					onChange={(event) => {
						setStatus(event.target.value);
						setPage(1);
					}}
				>
					<option value="all">All statuses</option>
					<option value="serving">Serving</option>
					<option value="degraded">Degraded</option>
					<option value="disabled">Disabled</option>
				</select>
				<select
					className="xp-select"
					value={sort}
					aria-label="Sort endpoints"
					onChange={(event) => setSort(event.target.value)}
				>
					<option value="name">Name</option>
					<option value="users">Users</option>
					<option value="probe">Probe latency</option>
				</select>
			</div>

			{state.endpoints.length === 0 ? (
				<PageState
					variant="empty"
					title="No endpoints yet"
					description="Create the first endpoint to make subscriptions useful."
					action={
						canWrite ? (
							<Button asChild>
								<Link to="/demo/endpoints/new">Create endpoint</Link>
							</Button>
						) : (
							<Button disabled>Create endpoint</Button>
						)
					}
				/>
			) : visible.length === 0 ? (
				<PageState
					variant="empty"
					title="No matching endpoints"
					description="Adjust search or status filters."
					action={
						<Button
							variant="secondary"
							onClick={() => {
								setQuery("");
								setStatus("all");
							}}
						>
							Clear filters
						</Button>
					}
				/>
			) : (
				<>
					<div className="xp-table-wrap">
						<table className="xp-table xp-table-zebra">
							<thead>
								<tr>
									<th>Endpoint</th>
									<th>Node</th>
									<th>Status</th>
									<th>Kind</th>
									<th>Users</th>
									<th>Probe</th>
								</tr>
							</thead>
							<tbody>
								{visible.map((endpoint) => {
									const node = state.nodes.find(
										(item) => item.id === endpoint.nodeId,
									);
									return (
										<tr key={endpoint.id}>
											<td>
												<Link
													className="font-medium hover:underline"
													to="/demo/endpoints/$endpointId"
													params={{ endpointId: endpoint.id }}
												>
													{endpoint.name}
												</Link>
												<p className="font-mono text-xs text-muted-foreground">
													{endpoint.id}
												</p>
											</td>
											<td>{node?.name ?? endpoint.nodeId}</td>
											<td>
												<Badge
													variant={endpointStatusVariant(endpoint.status)}
													size="sm"
												>
													{endpoint.status}
												</Badge>
											</td>
											<td className="max-w-[14rem] truncate text-xs">
												{endpointKindLabel(endpoint.kind)}
											</td>
											<td className="font-mono text-xs">
												{endpoint.assignedUserIds.length}
											</td>
											<td className="font-mono text-xs">
												{endpoint.probeLatencyMs === null
													? "not run"
													: `${endpoint.probeLatencyMs} ms`}
											</td>
										</tr>
									);
								})}
							</tbody>
						</table>
					</div>
					<div className="flex items-center justify-between gap-3">
						<p className="text-sm text-muted-foreground">
							Page {safePage} of {pages}, {filtered.length} endpoint(s)
						</p>
						<div className="flex gap-2">
							<Button
								variant="secondary"
								size="sm"
								disabled={safePage <= 1}
								onClick={() => setPage(Math.max(1, safePage - 1))}
							>
								Previous
							</Button>
							<Button
								variant="secondary"
								size="sm"
								disabled={safePage >= pages}
								onClick={() => setPage(Math.min(pages, safePage + 1))}
							>
								Next
							</Button>
						</div>
					</div>
				</>
			)}
		</div>
	);
}

export function DemoEndpointFormPage() {
	const navigate = useNavigate();
	const { state, createEndpoint } = useDemo();
	const { pushToast } = useToast();
	const [name, setName] = useState("seoul-reality-443");
	const [nodeId, setNodeId] = useState(state.nodes[0]?.id ?? "");
	const [kind, setKind] = useState<DemoEndpoint["kind"]>(
		"vless_reality_vision_tcp",
	);
	const [port, setPort] = useState("9443");
	const [serverNames, setServerNames] = useState("public.sn.files.1drv.com");
	const [submitted, setSubmitted] = useState(false);
	const [saving, setSaving] = useState(false);
	const canWrite = state.session?.role !== "viewer";

	const numericPort = Number(port);
	const duplicate = state.endpoints.some(
		(endpoint) => endpoint.nodeId === nodeId && endpoint.port === numericPort,
	);
	const serverNameList = serverNames
		.split(",")
		.map((item) => item.trim())
		.filter(Boolean);
	const error =
		name.trim().length < 3
			? "Name must be at least 3 characters."
			: !nodeId
				? "Node is required."
				: !Number.isInteger(numericPort) ||
						numericPort <= 0 ||
						numericPort > 65535
					? "Port must be an integer between 1 and 65535."
					: duplicate
						? "That node already has an endpoint on this port."
						: kind === "vless_reality_vision_tcp" && serverNameList.length === 0
							? "Reality endpoints need at least one serverName."
							: null;
	const dirty =
		name !== "seoul-reality-443" ||
		port !== "9443" ||
		serverNames !== "public.sn.files.1drv.com" ||
		kind !== "vless_reality_vision_tcp";

	return (
		<div className="space-y-6">
			<PageHeader
				title="New endpoint"
				description="Create a mock endpoint and confirm it appears in the list."
				meta={dirty ? <Badge variant="warning">unsaved changes</Badge> : null}
				actions={
					<Button asChild variant="ghost" size="sm">
						<Link to="/demo/endpoints">Back</Link>
					</Button>
				}
			/>

			<form
				className="xp-card"
				onSubmit={(event) => {
					event.preventDefault();
					setSubmitted(true);
					if (error || !canWrite) return;
					setSaving(true);
					window.setTimeout(() => {
						const endpoint = createEndpoint({
							name,
							nodeId,
							kind,
							port: numericPort,
							serverNames:
								kind === "vless_reality_vision_tcp" ? serverNameList : [],
						});
						setSaving(false);
						pushToast({ variant: "success", message: "Endpoint created." });
						navigate({
							to: "/demo/endpoints/$endpointId",
							params: { endpointId: endpoint.id },
						});
					}, 550);
				}}
			>
				<div className="xp-card-body">
					<div className="grid gap-4 md:grid-cols-2">
						<div className="xp-field-stack">
							<label
								className="text-sm font-medium"
								htmlFor="demo-endpoint-name"
							>
								Name
							</label>
							<Input
								id="demo-endpoint-name"
								value={name}
								onChange={(event) => setName(event.target.value)}
							/>
						</div>
						<div className="xp-field-stack">
							<label
								className="text-sm font-medium"
								htmlFor="demo-endpoint-node"
							>
								Node
							</label>
							<select
								id="demo-endpoint-node"
								className="xp-select"
								value={nodeId}
								onChange={(event) => setNodeId(event.target.value)}
							>
								{state.nodes.map((node) => (
									<option key={node.id} value={node.id}>
										{node.name} ({node.region})
									</option>
								))}
							</select>
						</div>
						<div className="xp-field-stack">
							<label
								className="text-sm font-medium"
								htmlFor="demo-endpoint-kind"
							>
								Kind
							</label>
							<select
								id="demo-endpoint-kind"
								className="xp-select"
								value={kind}
								onChange={(event) =>
									setKind(event.target.value as DemoEndpoint["kind"])
								}
							>
								<option value="vless_reality_vision_tcp">
									VLESS Reality Vision TCP
								</option>
								<option value="ss2022_2022_blake3_aes_128_gcm">
									SS2022 BLAKE3 AES-128-GCM
								</option>
							</select>
						</div>
						<div className="xp-field-stack">
							<label
								className="text-sm font-medium"
								htmlFor="demo-endpoint-port"
							>
								Port
							</label>
							<Input
								id="demo-endpoint-port"
								value={port}
								inputMode="numeric"
								onChange={(event) => setPort(event.target.value)}
							/>
						</div>
					</div>

					{kind === "vless_reality_vision_tcp" ? (
						<div className="xp-field-stack">
							<label
								className="text-sm font-medium"
								htmlFor="demo-endpoint-server-names"
							>
								serverNames
							</label>
							<Input
								id="demo-endpoint-server-names"
								value={serverNames}
								onChange={(event) => setServerNames(event.target.value)}
								placeholder="public.sn.files.1drv.com, oneclient.sfx.ms"
								className="font-mono"
							/>
							<span className="text-xs text-muted-foreground">
								Comma-separated Reality camouflage domains.
							</span>
						</div>
					) : null}

					{submitted && error ? (
						<div className="xp-alert xp-alert-error">{error}</div>
					) : null}
					{!canWrite ? (
						<div className="xp-alert xp-alert-warning">
							Viewer role cannot create demo records. Switch role on login.
						</div>
					) : null}

					<div className="flex flex-wrap justify-end gap-2 border-t border-border/70 pt-4">
						<Button asChild variant="ghost">
							<Link to="/demo/endpoints">Cancel</Link>
						</Button>
						<Button
							type="submit"
							loading={saving}
							disabled={!canWrite || saving}
						>
							Create endpoint
						</Button>
					</div>
				</div>
			</form>
		</div>
	);
}

export function DemoEndpointDetailsPage() {
	const { endpointId } = useParams({ from: "/demo/endpoints/$endpointId" });
	const navigate = useNavigate();
	const { state, createProbeRun } = useDemo();
	const { pushToast } = useToast();
	const [probing, setProbing] = useState(false);
	const endpoint = state.endpoints.find((item) => item.id === endpointId);
	const canWrite = state.session?.role !== "viewer";

	if (!endpoint) {
		return (
			<PageState
				variant="error"
				title="Endpoint not found"
				description="The selected demo endpoint does not exist in this seed."
				action={
					<Link className={buttonVariants()} to="/demo/endpoints">
						Back to endpoints
					</Link>
				}
			/>
		);
	}

	const node = state.nodes.find((item) => item.id === endpoint.nodeId);
	const users = state.users.filter((user) =>
		user.endpointIds.includes(endpoint.id),
	);
	const firstUser = users[0];

	return (
		<div className="space-y-6">
			<PageHeader
				title={endpoint.name}
				description={`${endpoint.id} on ${node?.name ?? endpoint.nodeId}`}
				meta={
					<Badge variant={endpointStatusVariant(endpoint.status)}>
						{endpoint.status}
					</Badge>
				}
				actions={
					<>
						<Button
							variant="secondary"
							loading={probing}
							disabled={!canWrite || probing}
							onClick={() => {
								if (!canWrite) return;
								setProbing(true);
								window.setTimeout(() => {
									const run = createProbeRun(endpoint.id);
									setProbing(false);
									pushToast({
										variant: run.status === "failed" ? "error" : "success",
										message:
											run.status === "failed"
												? "Probe completed with degraded result."
												: "Probe completed successfully.",
									});
									navigate({
										to: "/demo/endpoints/probe/runs/$runId",
										params: { runId: run.id },
									});
								}, 700);
							}}
						>
							Run probe
						</Button>
						<Button asChild variant="secondary" size="sm">
							<Link
								to="/demo/endpoints/$endpointId/probe"
								params={{ endpointId: endpoint.id }}
							>
								Probe stats
							</Link>
						</Button>
						<Button asChild variant="ghost" size="sm">
							<Link to="/demo/endpoints">Back</Link>
						</Button>
					</>
				}
			/>

			<div className="grid gap-4 md:grid-cols-3">
				<div className="xp-panel-muted p-4">
					<p className="text-xs uppercase tracking-[0.18em] text-muted-foreground">
						Kind
					</p>
					<p className="mt-2 text-sm font-medium">
						{endpointKindLabel(endpoint.kind)}
					</p>
				</div>
				<div className="xp-panel-muted p-4">
					<p className="text-xs uppercase tracking-[0.18em] text-muted-foreground">
						Port
					</p>
					<p className="mt-2 font-mono text-lg">{endpoint.port}</p>
				</div>
				<div className="xp-panel-muted p-4">
					<p className="text-xs uppercase tracking-[0.18em] text-muted-foreground">
						Last probe
					</p>
					<p className="mt-2 font-mono text-sm">
						{endpoint.probeLatencyMs === null
							? "not run"
							: `${endpoint.probeLatencyMs} ms at ${shortDate(endpoint.lastProbeAt)}`}
					</p>
				</div>
			</div>

			<section className="xp-card">
				<div className="xp-card-body">
					<h2 className="xp-card-title">Subscription preview</h2>
					{firstUser ? (
						<div className="rounded-2xl border border-border/70 bg-muted/35 p-4">
							<div className="flex flex-wrap items-start justify-between gap-3">
								<div>
									<p className="text-sm font-medium">{firstUser.displayName}</p>
									<p className="mt-1 break-all font-mono text-xs text-muted-foreground">
										{subscriptionUrl(firstUser.subscriptionToken)}
									</p>
								</div>
								<CopyButton
									text={subscriptionUrl(firstUser.subscriptionToken)}
									label="Copy URL"
									ariaLabel={`Copy subscription URL for ${firstUser.displayName}`}
								/>
							</div>
						</div>
					) : (
						<PageState
							variant="empty"
							title="No assigned users"
							description="Assign a user from the user details page to make this endpoint appear in a subscription."
							action={
								canWrite ? (
									<Button asChild>
										<Link to="/demo/users/new">Create user</Link>
									</Button>
								) : (
									<Button disabled>Create user</Button>
								)
							}
						/>
					)}
				</div>
			</section>
		</div>
	);
}

function probeStatusVariant(status: "ok" | "timeout" | "skipped") {
	if (status === "ok") return "success";
	if (status === "timeout") return "destructive";
	return "ghost";
}

export function DemoEndpointProbeStatsPage() {
	const { endpointId } = useParams({
		from: "/demo/endpoints/$endpointId/probe",
	});
	const navigate = useNavigate();
	const { state, createProbeRun } = useDemo();
	const { pushToast } = useToast();
	const [running, setRunning] = useState(false);
	const endpoint = state.endpoints.find((item) => item.id === endpointId);
	const canWrite = state.session?.role !== "viewer";

	if (!endpoint) {
		return (
			<PageState
				variant="error"
				title="Endpoint not found"
				description="The selected demo endpoint does not exist in this seed."
				action={
					<Link className={buttonVariants()} to="/demo/endpoints">
						Back to endpoints
					</Link>
				}
			/>
		);
	}

	const runs = state.probeRuns.filter((run) => run.endpointId === endpoint.id);
	const latest = runs[0] ?? null;

	return (
		<div className="space-y-6">
			<PageHeader
				title="Endpoint probe stats"
				description={`${endpoint.name} probe history and per-node samples.`}
				meta={
					<Badge variant={endpointStatusVariant(endpoint.status)}>
						{endpoint.status}
					</Badge>
				}
				actions={
					<>
						<Button
							loading={running}
							disabled={!canWrite || running}
							onClick={() => {
								setRunning(true);
								window.setTimeout(() => {
									const run = createProbeRun(endpoint.id);
									setRunning(false);
									pushToast({
										variant: run.status === "failed" ? "error" : "success",
										message: `Probe run ${run.id} completed.`,
									});
									navigate({
										to: "/demo/endpoints/probe/runs/$runId",
										params: { runId: run.id },
									});
								}, 600);
							}}
						>
							Run probe
						</Button>
						<Button asChild variant="ghost" size="sm">
							<Link
								to="/demo/endpoints/$endpointId"
								params={{ endpointId: endpoint.id }}
							>
								Back
							</Link>
						</Button>
					</>
				}
			/>

			<div className="grid gap-4 md:grid-cols-3">
				<div className="xp-panel-muted p-4">
					<p className="text-xs uppercase tracking-[0.18em] text-muted-foreground">
						Last latency
					</p>
					<p className="mt-2 font-mono text-lg">
						{endpoint.probeLatencyMs === null
							? "not run"
							: `${endpoint.probeLatencyMs} ms`}
					</p>
				</div>
				<div className="xp-panel-muted p-4">
					<p className="text-xs uppercase tracking-[0.18em] text-muted-foreground">
						Last run
					</p>
					<p className="mt-2 font-mono text-lg">
						{latest ? latest.id : "none"}
					</p>
				</div>
				<div className="xp-panel-muted p-4">
					<p className="text-xs uppercase tracking-[0.18em] text-muted-foreground">
						Samples
					</p>
					<p className="mt-2 font-mono text-lg">
						{latest ? latest.samples.length : 0}
					</p>
				</div>
			</div>

			{runs.length === 0 ? (
				<PageState
					variant="empty"
					title="No probe runs"
					description="Run a mock probe to populate per-node samples."
				/>
			) : (
				<section className="xp-card">
					<div className="xp-card-body">
						<h2 className="xp-card-title">Recent runs</h2>
						<div className="xp-table-wrap">
							<table className="xp-table xp-table-zebra">
								<thead>
									<tr>
										<th>Run</th>
										<th>Status</th>
										<th>Completed</th>
										<th>Samples</th>
									</tr>
								</thead>
								<tbody>
									{runs.map((run) => (
										<tr key={run.id}>
											<td>
												<Link
													className="font-mono text-xs hover:underline"
													to="/demo/endpoints/probe/runs/$runId"
													params={{ runId: run.id }}
												>
													{run.id}
												</Link>
											</td>
											<td>
												<Badge
													variant={
														run.status === "completed" ? "success" : "warning"
													}
												>
													{run.status}
												</Badge>
											</td>
											<td className="font-mono text-xs">
												{shortDate(run.completedAt)}
											</td>
											<td className="font-mono text-xs">
												{
													run.samples.filter((sample) => sample.status === "ok")
														.length
												}
												/{run.samples.length} ok
											</td>
										</tr>
									))}
								</tbody>
							</table>
						</div>
					</div>
				</section>
			)}
		</div>
	);
}

export function DemoEndpointProbeRunPage() {
	const { runId } = useParams({ from: "/demo/endpoints/probe/runs/$runId" });
	const { state } = useDemo();
	const run = state.probeRuns.find((item) => item.id === runId);

	if (!run) {
		return (
			<PageState
				variant="error"
				title="Probe run not found"
				description="The selected mock probe run is not available in this seed."
				action={
					<Link className={buttonVariants()} to="/demo/endpoints">
						Back to endpoints
					</Link>
				}
			/>
		);
	}

	const endpoint = state.endpoints.find((item) => item.id === run.endpointId);
	const okSamples = run.samples.filter((sample) => sample.status === "ok");
	const timeoutSamples = run.samples.filter(
		(sample) => sample.status === "timeout",
	);

	return (
		<div className="space-y-6">
			<PageHeader
				title="Endpoint probe run"
				description={`${run.id} for ${endpoint?.name ?? run.endpointId}`}
				meta={
					<Badge variant={run.status === "completed" ? "success" : "warning"}>
						{run.status}
					</Badge>
				}
				actions={
					<>
						{endpoint ? (
							<Button asChild variant="secondary" size="sm">
								<Link
									to="/demo/endpoints/$endpointId/probe"
									params={{ endpointId: endpoint.id }}
								>
									Probe stats
								</Link>
							</Button>
						) : null}
						<Button asChild variant="ghost" size="sm">
							<Link to="/demo/endpoints">Back</Link>
						</Button>
					</>
				}
			/>

			<div className="grid gap-4 md:grid-cols-3">
				<div className="xp-panel-muted p-4">
					<p className="text-xs uppercase tracking-[0.18em] text-muted-foreground">
						OK samples
					</p>
					<p className="mt-2 font-mono text-lg">{okSamples.length}</p>
				</div>
				<div className="xp-panel-muted p-4">
					<p className="text-xs uppercase tracking-[0.18em] text-muted-foreground">
						Timeouts
					</p>
					<p className="mt-2 font-mono text-lg">{timeoutSamples.length}</p>
				</div>
				<div className="xp-panel-muted p-4">
					<p className="text-xs uppercase tracking-[0.18em] text-muted-foreground">
						Completed
					</p>
					<p className="mt-2 font-mono text-sm">{shortDate(run.completedAt)}</p>
				</div>
			</div>

			<section className="xp-card">
				<div className="xp-card-body">
					<h2 className="xp-card-title">Node samples</h2>
					<div className="xp-table-wrap">
						<table className="xp-table xp-table-zebra">
							<thead>
								<tr>
									<th>Node</th>
									<th>Status</th>
									<th>Latency</th>
									<th>Message</th>
								</tr>
							</thead>
							<tbody>
								{run.samples.map((sample) => {
									const node = state.nodes.find(
										(item) => item.id === sample.nodeId,
									);
									return (
										<tr key={sample.nodeId}>
											<td>
												<p className="font-medium">
													{node?.name ?? sample.nodeId}
												</p>
												<p className="font-mono text-xs text-muted-foreground">
													{sample.nodeId}
												</p>
											</td>
											<td>
												<Badge variant={probeStatusVariant(sample.status)}>
													{sample.status}
												</Badge>
											</td>
											<td className="font-mono text-xs">
												{sample.latencyMs === null
													? "-"
													: `${sample.latencyMs} ms`}
											</td>
											<td className="text-sm">{sample.message}</td>
										</tr>
									);
								})}
							</tbody>
						</table>
					</div>
				</div>
			</section>
		</div>
	);
}
