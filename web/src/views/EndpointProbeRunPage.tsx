import { useQuery } from "@tanstack/react-query";
import { Link, useNavigate, useParams } from "@tanstack/react-router";

import {
	type AdminEndpointProbeRunNodeStatus,
	fetchAdminEndpointProbeRunStatus,
} from "../api/adminEndpointProbes";
import {
	type EndpointProbeStatus,
	fetchAdminEndpoints,
} from "../api/adminEndpoints";
import { isBackendApiError } from "../api/backendError";
import { Button } from "../components/Button";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
import { ResourceTable } from "../components/ResourceTable";
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

export function EndpointProbeRunPage() {
	const { runId } = useParams({ from: "/app/endpoints/probe/runs/$runId" });
	const navigate = useNavigate();
	const adminToken = readAdminToken();

	const statusQuery = useQuery({
		queryKey: ["adminEndpointProbeRunStatus", adminToken, runId],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) =>
			fetchAdminEndpointProbeRunStatus(adminToken, runId, signal),
		refetchInterval: (query) =>
			query.state.data?.status === "running" ? 1000 : false,
	});

	const endpointsQuery = useQuery({
		queryKey: ["adminEndpoints", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminEndpoints(adminToken, signal),
		refetchInterval: () =>
			statusQuery.data?.status === "running" ? 2000 : false,
	});

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
	const hour = data.hour ?? "-";
	const configHash = data.config_hash
		? `${data.config_hash.slice(0, 12)}…`
		: "-";

	const overallBadgeClass =
		data.status === "running"
			? "badge badge-info"
			: data.status === "finished"
				? "badge badge-success"
				: data.status === "failed"
					? "badge badge-error"
					: data.status === "unknown"
						? "badge badge-neutral"
						: "badge";

	const overallLabel =
		data.status === "running"
			? "Running"
			: data.status === "finished"
				? "Finished"
				: data.status === "failed"
					? "Failed"
					: data.status === "unknown"
						? "Unknown"
						: "Not found";

	const resultsContent = (() => {
		if (endpointsQuery.isLoading) {
			return (
				<div className="text-sm opacity-70">
					Loading endpoint results (latency)...
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
					{ key: "status", label: "Status (this hour)" },
					{ key: "latency", label: "Latency (p50 ms)" },
					{ key: "checkedAt", label: "Checked at" },
				]}
			>
				{endpoints.map((endpoint) => {
					const hourSlot =
						data.hour && endpoint.probe?.slots
							? (endpoint.probe.slots.find((slot) => slot.hour === data.hour) ??
								null)
							: null;
					const hourStatus: EndpointProbeStatus = hourSlot?.status ?? "missing";
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
							<td>
								<span className={endpointStatusBadgeClass(hourStatus)}>
									{endpointStatusLabel(hourStatus)}
								</span>
							</td>
							<td className="font-mono text-xs">
								{endpoint.probe?.latest_latency_ms_p50 ?? "-"}
							</td>
							<td className="font-mono text-xs opacity-70">
								{endpoint.probe?.latest_checked_at ?? "-"}
							</td>
						</tr>
					);
				})}
			</ResourceTable>
		);
	})();

	return (
		<div className="space-y-6">
			<PageHeader
				title="Endpoint probe run"
				description="Cluster-wide probe progress across nodes."
				meta={
					<div className="flex flex-wrap items-center gap-2">
						<span className={overallBadgeClass}>{overallLabel}</span>
						<span className="badge badge-ghost font-mono text-xs">{hour}</span>
						<span className="badge badge-ghost font-mono text-xs">
							cfg {configHash}
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
							This page auto-refreshes while the run is running.
						</div>
					</div>
				</div>
			</div>

			<ResourceTable
				headers={[
					{ key: "node", label: "Node" },
					{ key: "status", label: "Status" },
					{ key: "progress", label: "Progress" },
					{ key: "updated", label: "Updated" },
					{ key: "error", label: "Error" },
				]}
			>
				{data.nodes.map((node) => {
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

			<div className="card bg-base-100 shadow">
				<div className="card-body space-y-4">
					<h2 className="card-title">Latency (live)</h2>
					<p className="text-sm opacity-70">
						Latency results are aggregated from node samples as they are
						reported.
					</p>
					{resultsContent}
				</div>
			</div>
		</div>
	);
}
