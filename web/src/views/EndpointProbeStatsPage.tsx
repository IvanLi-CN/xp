import { useQuery } from "@tanstack/react-query";
import { Link, useParams } from "@tanstack/react-router";
import { useEffect, useMemo, useState } from "react";

import { fetchAdminEndpointProbeHistory } from "../api/adminEndpointProbes";
import { fetchAdminEndpoint } from "../api/adminEndpoints";
import type { EndpointProbeStatus } from "../api/adminEndpoints";
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

function statusClass(status: EndpointProbeStatus): string {
	switch (status) {
		case "up":
			return "bg-success";
		case "degraded":
			return "bg-warning";
		case "down":
			return "bg-error";
		default:
			return "bg-base-300";
	}
}

function statusLabel(status: EndpointProbeStatus): string {
	switch (status) {
		case "up":
			return "Up";
		case "degraded":
			return "Degraded";
		case "down":
			return "Down";
		default:
			return "Missing";
	}
}

export function EndpointProbeStatsPage() {
	const { endpointId } = useParams({
		from: "/app/endpoints/$endpointId/probe",
	});
	const adminToken = readAdminToken();

	const endpointQuery = useQuery({
		queryKey: ["adminEndpoint", adminToken, endpointId],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminEndpoint(adminToken, endpointId, signal),
	});

	const historyQuery = useQuery({
		queryKey: ["adminEndpointProbeHistory", adminToken, endpointId],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) =>
			fetchAdminEndpointProbeHistory(adminToken, endpointId, 24, signal),
	});

	const slots = historyQuery.data?.slots ?? [];

	const defaultSelectedHour = useMemo(() => {
		// Pick the latest slot with data; fall back to the last slot.
		const withData = [...slots].reverse().find((slot) => slot.sample_count > 0);
		return withData?.hour ?? slots[slots.length - 1]?.hour ?? null;
	}, [slots]);

	const [selectedHour, setSelectedHour] = useState<string | null>(null);

	useEffect(() => {
		if (!selectedHour && defaultSelectedHour) {
			setSelectedHour(defaultSelectedHour);
		}
	}, [defaultSelectedHour, selectedHour]);

	const selected = useMemo(() => {
		if (!selectedHour) return null;
		return slots.find((slot) => slot.hour === selectedHour) ?? null;
	}, [slots, selectedHour]);

	if (adminToken.length === 0) {
		return (
			<PageState
				variant="empty"
				title="Admin token required"
				description="Set an admin token to load probe stats."
				action={
					<Link className="btn btn-primary" to="/login">
						Go to login
					</Link>
				}
			/>
		);
	}

	if (endpointQuery.isLoading || historyQuery.isLoading) {
		return (
			<PageState
				variant="loading"
				title="Loading probe stats"
				description="Fetching probe history from the xp API."
			/>
		);
	}

	if (endpointQuery.isError || historyQuery.isError) {
		const description = formatErrorMessage(
			(endpointQuery.error ?? historyQuery.error) as unknown,
		);
		return (
			<PageState
				variant="error"
				title="Failed to load probe stats"
				description={description}
				action={
					<Button
						variant="secondary"
						onClick={() => {
							endpointQuery.refetch();
							historyQuery.refetch();
						}}
					>
						Retry
					</Button>
				}
			/>
		);
	}

	const endpoint = endpointQuery.data;
	if (!endpoint) {
		return (
			<PageState
				variant="empty"
				title="Endpoint not found"
				description="The requested endpoint does not exist."
				action={
					<Link className="btn btn-primary" to="/endpoints">
						Back to endpoints
					</Link>
				}
			/>
		);
	}

	return (
		<div className="space-y-6">
			<PageHeader
				title="Endpoint probe stats"
				description={`Endpoint ${endpoint.tag} (${endpoint.endpoint_id})`}
				actions={
					<div className="flex gap-2">
						<Link className="btn btn-ghost btn-sm" to="/endpoints">
							Back
						</Link>
						<Button
							variant="secondary"
							loading={historyQuery.isFetching}
							onClick={() => historyQuery.refetch()}
						>
							Refresh
						</Button>
					</div>
				}
			/>

			<div className="card bg-base-100 shadow">
				<div className="card-body space-y-4">
					<h2 className="card-title">Last 24 hours</h2>
					<p className="text-sm opacity-70">
						Each bar represents one hour bucket (UTC). Click a bar to inspect
						per-node results.
					</p>

					<div className="flex flex-wrap items-end gap-1">
						{slots.map((slot) => {
							const isSelected = slot.hour === selectedHour;
							return (
								<button
									key={slot.hour}
									type="button"
									title={`${slot.hour} • ${statusLabel(slot.status)} • ${slot.ok_count}/${slot.sample_count}`}
									onClick={() => setSelectedHour(slot.hour)}
									className={[
										"h-6 w-2 rounded-sm",
										statusClass(slot.status),
										slot.status === "missing" ? "opacity-50" : "",
										isSelected
											? "ring ring-primary ring-offset-2 ring-offset-base-100"
											: "",
									]
										.filter(Boolean)
										.join(" ")}
								/>
							);
						})}
					</div>
				</div>
			</div>

			<div className="grid gap-6 lg:grid-cols-2">
				<div className="card bg-base-100 shadow">
					<div className="card-body space-y-2">
						<h2 className="card-title">Selected hour</h2>
						{selected ? (
							<div className="space-y-1 text-sm">
								<p>
									Hour: <span className="font-mono">{selected.hour}</span>
								</p>
								<p>
									Status:{" "}
									<span className="font-mono">
										{statusLabel(selected.status)}
									</span>
								</p>
								<p>
									OK:{" "}
									<span className="font-mono">
										{selected.ok_count}/{selected.sample_count}
									</span>
								</p>
								<p>
									Latency (p50):{" "}
									<span className="font-mono">
										{selected.latency_ms_p50 ?? "-"} ms
									</span>
								</p>
								<p>
									Latency (p95):{" "}
									<span className="font-mono">
										{selected.latency_ms_p95 ?? "-"} ms
									</span>
								</p>
							</div>
						) : (
							<p className="text-sm opacity-70">No slot selected.</p>
						)}
					</div>
				</div>

				<div className="card bg-base-100 shadow">
					<div className="card-body space-y-2">
						<h2 className="card-title">Probe config</h2>
						<p className="text-sm opacity-70">
							Probe config hash is recorded per node sample.
						</p>
						<p className="text-sm">
							Expected nodes:{" "}
							<span className="font-mono">
								{historyQuery.data?.expected_nodes}
							</span>
						</p>
					</div>
				</div>
			</div>

			<div className="card bg-base-100 shadow">
				<div className="card-body space-y-4">
					<h2 className="card-title">Per-node results</h2>
					{selected ? (
						selected.by_node.length === 0 ? (
							<p className="text-sm opacity-70">No samples for this hour.</p>
						) : (
							<ResourceTable
								headers={[
									{ key: "node", label: "Node" },
									{ key: "ok", label: "OK" },
									{ key: "latency", label: "Latency (ms)" },
									{ key: "checkedAt", label: "Checked at" },
									{ key: "error", label: "Error" },
								]}
							>
								{selected.by_node.map((sample) => (
									<tr key={sample.node_id}>
										<td className="font-mono text-xs">{sample.node_id}</td>
										<td>
											<span
												className={[
													"badge badge-sm",
													sample.ok ? "badge-success" : "badge-error",
												].join(" ")}
											>
												{sample.ok ? "OK" : "FAIL"}
											</span>
										</td>
										<td className="font-mono text-xs">
											{sample.latency_ms ?? "-"}
										</td>
										<td className="font-mono text-xs">{sample.checked_at}</td>
										<td className="font-mono text-xs">{sample.error ?? "-"}</td>
									</tr>
								))}
							</ResourceTable>
						)
					) : (
						<p className="text-sm opacity-70">
							Select an hour slot to view results.
						</p>
					)}
				</div>
			</div>
		</div>
	);
}
