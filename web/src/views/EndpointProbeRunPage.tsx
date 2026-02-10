import { useQuery } from "@tanstack/react-query";
import { Link, useNavigate, useParams } from "@tanstack/react-router";
import { useEffect, useMemo, useState } from "react";

import {
	type AdminEndpointProbeRunNodeStatus,
	AdminEndpointProbeRunSseEndpointSampleSchema,
	AdminEndpointProbeRunSseHelloSchema,
	AdminEndpointProbeRunSseLaggedSchema,
	AdminEndpointProbeRunSseNodeErrorSchema,
	AdminEndpointProbeRunSseNodeProgressSchema,
	AdminEndpointProbeRunSseNotFoundSchema,
	type AdminEndpointProbeRunStatusResponse,
	type EndpointProbeAppendSample,
	fetchAdminEndpointProbeRunStatus,
} from "../api/adminEndpointProbes";
import {
	type EndpointProbeStatus,
	fetchAdminEndpoints,
} from "../api/adminEndpoints";
import { isBackendApiError } from "../api/backendError";
import { startSseStream } from "../api/sse";
import { Button } from "../components/Button";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
import { ResourceTable } from "../components/ResourceTable";
import { useToast } from "../components/Toast";
import { readAdminToken } from "../components/auth";

function formatErrorMessage(error: unknown): string {
	if (isBackendApiError(error)) {
		const code = error.code ? ` ${error.code}` : "";
		return `${error.status}${code}: ${error.message}`;
	}
	if (error instanceof Error) return error.message;
	return String(error);
}

function statusBadgeClass(status: AdminEndpointProbeRunNodeStatus): string {
	switch (status) {
		case "running":
			return "badge badge-info";
		case "finished":
			return "badge badge-success";
		case "failed":
			return "badge badge-error";
		case "busy":
			return "badge badge-warning";
		case "not_found":
			return "badge";
		case "unknown":
			return "badge badge-neutral";
	}
}

function statusLabel(status: AdminEndpointProbeRunNodeStatus): string {
	switch (status) {
		case "running":
			return "Running";
		case "finished":
			return "Finished";
		case "failed":
			return "Failed";
		case "busy":
			return "Busy";
		case "not_found":
			return "Not found";
		case "unknown":
			return "Unknown";
	}
}

function endpointStatusBadgeClass(status: EndpointProbeStatus): string {
	switch (status) {
		case "up":
			return "badge badge-success";
		case "degraded":
			return "badge badge-warning";
		case "down":
			return "badge badge-error";
		case "missing":
			return "badge badge-neutral";
	}
}

function endpointStatusLabel(status: EndpointProbeStatus): string {
	switch (status) {
		case "up":
			return "Up";
		case "degraded":
			return "Degraded";
		case "down":
			return "Down";
		case "missing":
			return "Missing";
	}
}

type NodeRunner = AdminEndpointProbeRunStatusResponse["nodes"][number];

type HasUpdatedAt = { updated_at: string };

function pickNewestByUpdatedAt<T extends HasUpdatedAt>(
	a: T | undefined,
	b: T | undefined,
): T | undefined {
	if (!a) return b;
	if (!b) return a;

	const aTs = Date.parse(a.updated_at);
	const bTs = Date.parse(b.updated_at);
	if (Number.isNaN(aTs) || Number.isNaN(bTs)) {
		return a.updated_at >= b.updated_at ? a : b;
	}
	return aTs >= bTs ? a : b;
}

function percentile(values: number[], p: number): number | null {
	if (values.length === 0) return null;
	const sorted = [...values].sort((a, b) => a - b);
	const idx = Math.round((sorted.length - 1) * Math.max(0, Math.min(1, p)));
	return sorted[idx] ?? null;
}

function computeEndpointProbeStatus(args: {
	expectedNodes: number;
	sampleCount: number;
	okCount: number;
}): EndpointProbeStatus {
	if (args.expectedNodes === 0) return "missing";
	if (args.sampleCount === 0) return "missing";
	if (args.sampleCount < args.expectedNodes) return "missing";
	if (args.okCount === 0) return "down";
	if (args.okCount >= args.expectedNodes) return "up";
	return "degraded";
}

export function EndpointProbeRunPage() {
	const { runId } = useParams({ from: "/app/endpoints/probe/runs/$runId" });
	const navigate = useNavigate();
	const adminToken = readAdminToken();
	const { pushToast } = useToast();

	const [sseConnected, setSseConnected] = useState(false);
	const [nodeRunnersById, setNodeRunnersById] = useState<
		Record<string, NodeRunner>
	>({});
	const [endpointSamplesById, setEndpointSamplesById] = useState<
		Record<string, Record<string, EndpointProbeAppendSample>>
	>({});
	const [liveHour, setLiveHour] = useState<string | null>(null);
	const [liveConfigHash, setLiveConfigHash] = useState<string | null>(null);
	const [autoRefreshedResults, setAutoRefreshedResults] = useState(false);

	const statusQuery = useQuery({
		queryKey: ["adminEndpointProbeRunStatus", adminToken, runId],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) =>
			fetchAdminEndpointProbeRunStatus(adminToken, runId, signal),
	});

	const endpointsQuery = useQuery({
		queryKey: ["adminEndpoints", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminEndpoints(adminToken, signal),
	});

	useEffect(() => {
		const data = statusQuery.data;
		if (!data) return;

		setNodeRunnersById((prev) => {
			const next: Record<string, NodeRunner> = {};
			for (const node of data.nodes) {
				const existing = prev[node.node_id];
				next[node.node_id] = {
					...node,
					progress: pickNewestByUpdatedAt(existing?.progress, node.progress),
					current: pickNewestByUpdatedAt(existing?.current, node.current),
				};
			}
			return next;
		});
	}, [statusQuery.data]);

	useEffect(() => {
		setSseConnected(false);
		setEndpointSamplesById({});
		setLiveHour(null);
		setLiveConfigHash(null);
		setAutoRefreshedResults(false);

		if (adminToken.length === 0) return;

		const handle = startSseStream({
			url: `/api/admin/endpoints/probe/runs/${encodeURIComponent(runId)}/events`,
			headers: {
				Authorization: `Bearer ${adminToken}`,
			},
			onMessage: ({ event, data }) => {
				let json: unknown;
				try {
					json = JSON.parse(data);
				} catch {
					return;
				}

				if (event === "hello") {
					const parsed = AdminEndpointProbeRunSseHelloSchema.safeParse(json);
					if (parsed.success) setSseConnected(true);
					return;
				}

				if (event === "progress") {
					const parsed =
						AdminEndpointProbeRunSseNodeProgressSchema.safeParse(json);
					if (!parsed.success) return;

					const { node_id, progress } = parsed.data;
					setLiveHour(progress.hour);
					setLiveConfigHash(progress.config_hash);
					setNodeRunnersById((prev) => {
						const existing = prev[node_id];
						const status: AdminEndpointProbeRunNodeStatus =
							progress.status === "running"
								? "running"
								: progress.status === "finished"
									? "finished"
									: "failed";
						const nextNode: NodeRunner = {
							node_id,
							status,
							progress,
							current: existing?.current,
							error: progress.error ?? existing?.error,
						};
						return { ...prev, [node_id]: nextNode };
					});
					return;
				}

				if (event === "sample") {
					const parsed =
						AdminEndpointProbeRunSseEndpointSampleSchema.safeParse(json);
					if (!parsed.success) return;

					const { node_id, sample } = parsed.data;
					setEndpointSamplesById((prev) => {
						const endpointId = sample.endpoint_id;
						const byNode = prev[endpointId] ?? {};
						return {
							...prev,
							[endpointId]: {
								...byNode,
								[node_id]: sample,
							},
						};
					});
					return;
				}

				if (event === "node_error") {
					const parsed =
						AdminEndpointProbeRunSseNodeErrorSchema.safeParse(json);
					if (!parsed.success) return;
					pushToast({
						variant: "error",
						message: `SSE node error (${parsed.data.node_id}): ${parsed.data.error}`,
					});
					return;
				}

				if (event === "lagged") {
					const parsed = AdminEndpointProbeRunSseLaggedSchema.safeParse(json);
					if (!parsed.success) return;
					pushToast({
						variant: "info",
						message: `SSE lagged (${parsed.data.node_id}): missed ${parsed.data.missed} events`,
					});
					return;
				}

				if (event === "not_found") {
					const parsed = AdminEndpointProbeRunSseNotFoundSchema.safeParse(json);
					if (!parsed.success) return;
					setNodeRunnersById((prev) => {
						const existing = prev[parsed.data.node_id];
						const nextNode: NodeRunner = {
							node_id: parsed.data.node_id,
							status: "not_found",
							progress: existing?.progress,
							current: existing?.current,
							error: existing?.error,
						};
						return { ...prev, [parsed.data.node_id]: nextNode };
					});
				}
			},
			onOpen: () => {
				setSseConnected(true);
			},
			onError: (err) => {
				pushToast({
					variant: "error",
					message: `SSE connection failed: ${formatErrorMessage(err)}`,
				});
			},
			onClose: () => {
				setSseConnected(false);
			},
		});

		return () => handle.close();
	}, [adminToken, runId, pushToast]);

	const nodesForUi = useMemo(() => {
		const data = statusQuery.data;
		if (!data) return [];

		const liveNodes = Object.values(nodeRunnersById);
		if (liveNodes.length > 0) {
			return liveNodes
				.slice()
				.sort((a, b) => a.node_id.localeCompare(b.node_id));
		}

		return data.nodes;
	}, [nodeRunnersById, statusQuery.data]);

	const overallStatus = useMemo(() => {
		const data = statusQuery.data;
		if (!data) return "unknown";

		const anyRunning = nodesForUi.some((n) => n.status === "running");
		const anyFailed = nodesForUi.some((n) => n.status === "failed");
		const anyFinished = nodesForUi.some((n) => n.status === "finished");
		const anyUnknown = nodesForUi.some((n) => n.status === "unknown");

		if (anyRunning) return "running";
		if (anyFailed) return "failed";
		if (anyFinished) return "finished";
		if (anyUnknown) return "unknown";
		return data.status;
	}, [nodesForUi, statusQuery.data]);

	useEffect(() => {
		if (autoRefreshedResults) return;
		if (overallStatus !== "finished" && overallStatus !== "failed") return;

		setAutoRefreshedResults(true);
		statusQuery.refetch();
		endpointsQuery.refetch();
	}, [autoRefreshedResults, overallStatus, endpointsQuery, statusQuery]);

	if (adminToken.length === 0) {
		return (
			<PageState
				variant="empty"
				title="Admin token required"
				description="Set an admin token to view probe progress."
				action={
					<Link className="btn btn-primary" to="/login">
						Go to login
					</Link>
				}
			/>
		);
	}

	const actions = (
		<>
			<Button
				variant="ghost"
				onClick={() => navigate({ to: "/endpoints" })}
				disabled={statusQuery.isLoading}
			>
				Back
			</Button>
			<Button
				variant="secondary"
				loading={statusQuery.isFetching || endpointsQuery.isFetching}
				onClick={() => {
					statusQuery.refetch();
					endpointsQuery.refetch();
				}}
			>
				Refresh
			</Button>
		</>
	);

	if (statusQuery.isLoading) {
		return (
			<div className="space-y-6">
				<PageHeader
					title="Endpoint probe run"
					description={`Run ID: ${runId}`}
					actions={<div className="flex flex-wrap gap-2">{actions}</div>}
				/>
				<PageState
					variant="loading"
					title="Loading probe status"
					description="Fetching run progress from the xp API."
				/>
			</div>
		);
	}

	if (statusQuery.isError) {
		return (
			<div className="space-y-6">
				<PageHeader
					title="Endpoint probe run"
					description={`Run ID: ${runId}`}
					actions={<div className="flex flex-wrap gap-2">{actions}</div>}
				/>
				<PageState
					variant="error"
					title="Failed to load probe status"
					description={formatErrorMessage(statusQuery.error)}
					action={
						<Button variant="secondary" onClick={() => statusQuery.refetch()}>
							Retry
						</Button>
					}
				/>
			</div>
		);
	}

	const data = statusQuery.data;
	if (!data) {
		return (
			<div className="space-y-6">
				<PageHeader
					title="Endpoint probe run"
					description={`Run ID: ${runId}`}
					actions={<div className="flex flex-wrap gap-2">{actions}</div>}
				/>
				<PageState
					variant="loading"
					title="Loading probe status"
					description="Waiting for the xp API response."
				/>
			</div>
		);
	}
	const hour = liveHour ?? data.hour ?? "-";
	const configHash = liveConfigHash ?? data.config_hash ?? "";
	const configHashShort = configHash ? `${configHash.slice(0, 12)}…` : "-";

	const overallBadgeClass =
		overallStatus === "running"
			? "badge badge-info"
			: overallStatus === "finished"
				? "badge badge-success"
				: overallStatus === "failed"
					? "badge badge-error"
					: overallStatus === "unknown"
						? "badge badge-neutral"
						: "badge";

	const overallLabel =
		overallStatus === "running"
			? "Running"
			: overallStatus === "finished"
				? "Finished"
				: overallStatus === "failed"
					? "Failed"
					: overallStatus === "unknown"
						? "Unknown"
						: "Not found";

	const expectedNodes = data.nodes.length;
	const runHourKey = liveHour ?? data.hour ?? null;

	const resultsContent = (() => {
		if (endpointsQuery.isLoading) {
			return (
				<div className="text-sm opacity-70">
					Loading endpoint results (status/latency)...
				</div>
			);
		}

		if (endpointsQuery.isError) {
			return (
				<div className="text-sm text-error">
					Failed to load endpoint results:{" "}
					{formatErrorMessage(endpointsQuery.error)}
				</div>
			);
		}

		const endpoints = endpointsQuery.data?.items ?? [];
		if (endpoints.length === 0) {
			return (
				<div className="text-sm opacity-70">
					No endpoints found for this cluster.
				</div>
			);
		}

		return (
			<ResourceTable
				headers={[
					{ key: "endpoint", label: "Endpoint" },
					{ key: "result", label: "Result" },
				]}
			>
				{endpoints.map((endpoint) => {
					const samplesByNode = endpointSamplesById[endpoint.endpoint_id] ?? {};
					const samples = Object.values(samplesByNode);

					const okSamples = samples.filter((s) => s.ok);
					const okCount = okSamples.length;
					const sampleCount = samples.length;
					const latencies = okSamples
						.map((s) => s.latency_ms)
						.filter((v): v is number => typeof v === "number");
					const latencyP50 = percentile(latencies, 0.5);
					const checkedAtFromSse =
						samples
							.map((s) => s.checked_at)
							.sort()
							.pop() ?? null;

					const slot =
						runHourKey && endpoint.probe?.slots
							? (endpoint.probe.slots.find((s) => s.hour === runHourKey) ??
								null)
							: null;

					const hourStatus: EndpointProbeStatus =
						sampleCount > 0
							? computeEndpointProbeStatus({
									expectedNodes,
									sampleCount,
									okCount,
								})
							: (slot?.status ?? "missing");
					const latencyMs = latencyP50 ?? slot?.latency_ms_p50 ?? null;
					const checkedAt = checkedAtFromSse ?? slot?.checked_at ?? null;

					const resultTooltip = checkedAt
						? `Checked at: ${checkedAt}`
						: undefined;
					return (
						<tr key={endpoint.endpoint_id}>
							<td className="space-y-1">
								<Link
									className="link link-primary font-mono text-xs"
									to="/endpoints/$endpointId"
									params={{ endpointId: endpoint.endpoint_id }}
								>
									{endpoint.tag}
								</Link>
								<div className="font-mono text-xs opacity-70">
									{endpoint.endpoint_id}
								</div>
								<Link
									className="link link-secondary text-xs"
									to="/endpoints/$endpointId/probe"
									params={{ endpointId: endpoint.endpoint_id }}
								>
									View stats
								</Link>
							</td>
							<td title={resultTooltip}>
								<div className="flex flex-wrap items-center gap-2">
									<span className={endpointStatusBadgeClass(hourStatus)}>
										{endpointStatusLabel(hourStatus)}
									</span>
									<span className="font-mono text-xs">
										{latencyMs ?? "-"}{" "}
										{typeof latencyMs === "number" ? (
											<span className="opacity-70">ms</span>
										) : null}
									</span>
								</div>
							</td>
						</tr>
					);
				})}
			</ResourceTable>
		);
	})();

	const nodeRunnersContent = (
		<ResourceTable
			headers={[
				{ key: "node", label: "Node" },
				{ key: "status", label: "Status" },
				{ key: "progress", label: "Progress" },
				{ key: "updated", label: "Updated" },
				{ key: "error", label: "Error" },
			]}
		>
			{nodesForUi.map((node) => {
				const progress = node.progress;
				const done = progress?.endpoints_done ?? 0;
				const total = progress?.endpoints_total ?? 0;

				const progressLabel = progress
					? `${done}/${total}`
					: node.status === "busy" && node.current
						? `busy: ${node.current.run_id}`
						: "-";

				const progressMax = total > 0 ? total : 1;
				const error =
					node.error ??
					node.progress?.error ??
					(node.status === "failed" ? "-" : "");

				return (
					<tr key={node.node_id}>
						<td className="font-mono text-xs">{node.node_id}</td>
						<td>
							<span className={statusBadgeClass(node.status)}>
								{statusLabel(node.status)}
							</span>
						</td>
						<td className="min-w-48">
							<div className="space-y-1">
								<div className="font-mono text-xs opacity-70">
									{progressLabel}
								</div>
								<progress
									className="progress progress-primary"
									value={done}
									max={progressMax}
								/>
							</div>
						</td>
						<td className="font-mono text-xs opacity-70">
							{progress?.updated_at ?? "-"}
						</td>
						<td className="font-mono text-xs text-error">{error || "-"}</td>
					</tr>
				);
			})}
		</ResourceTable>
	);

	return (
		<div className="space-y-6">
			<PageHeader
				title="Endpoint probe run"
				description="Cluster-wide endpoint test progress."
				meta={
					<div className="flex flex-wrap items-center gap-2">
						<span className={overallBadgeClass}>{overallLabel}</span>
						{sseConnected ? (
							<span className="badge badge-ghost text-xs">live</span>
						) : null}
						<span className="badge badge-ghost font-mono text-xs">{hour}</span>
						<span className="badge badge-ghost font-mono text-xs">
							cfg {configHashShort}
						</span>
					</div>
				}
				actions={<div className="flex flex-wrap gap-2">{actions}</div>}
			/>

			<div className="rounded-box border border-base-300 bg-base-100 p-4 shadow-sm">
				<div className="grid gap-3 md:grid-cols-2">
					<div>
						<div className="text-sm opacity-70">Run ID</div>
						<div className="font-mono text-xs break-all">{data.run_id}</div>
					</div>
					<div>
						<div className="text-sm opacity-70">Note</div>
						<div className="text-sm opacity-70">
							This page updates live via SSE while the run is running.
						</div>
					</div>
				</div>
			</div>

			<div className="card bg-base-100 shadow">
				<div className="card-body space-y-4">
					<h2 className="card-title">Endpoints (live)</h2>
					<p className="text-sm opacity-70">
						This run tests endpoints. Status/latency is aggregated from node
						samples as they are reported.
					</p>
					{resultsContent}
				</div>
			</div>

			<details className="collapse collapse-arrow border border-base-300 bg-base-100 shadow-sm">
				<summary className="collapse-title text-sm font-medium">
					Node runners (progress)
				</summary>
				<div className="collapse-content pt-0">{nodeRunnersContent}</div>
			</details>
		</div>
	);
}
