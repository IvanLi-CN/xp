import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";

import {
	type AdminIpGeoDbNodeStatus,
	fetchAdminIpGeoDb,
	patchAdminIpGeoDb,
	triggerAdminIpGeoDbUpdate,
} from "../api/adminIpGeoDb";
import { isBackendApiError } from "../api/backendError";
import { Button } from "../components/Button";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
import { useToast } from "../components/Toast";
import { readAdminToken } from "../components/auth";

function formatError(error: unknown): string {
	if (isBackendApiError(error)) {
		const code = error.code ? ` ${error.code}` : "";
		return `${error.status}${code}: ${error.message}`;
	}
	return String(error);
}

function formatDateTime(value: string | null | undefined): string {
	if (!value) return "-";
	const date = new Date(value);
	if (Number.isNaN(date.getTime())) return value;
	return date.toLocaleString();
}

function modeLabel(mode: AdminIpGeoDbNodeStatus["mode"]): string {
	switch (mode) {
		case "managed":
			return "Managed";
		case "external_override":
			return "External override";
		case "missing":
			return "Missing";
	}
}

function modeBadgeClass(mode: AdminIpGeoDbNodeStatus["mode"]): string {
	switch (mode) {
		case "managed":
			return "badge-success";
		case "external_override":
			return "badge-info";
		case "missing":
			return "badge-warning";
	}
}

function SummaryChip({
	label,
	value,
	tone = "neutral",
}: {
	label: string;
	value: string;
	tone?: "neutral" | "warning";
}) {
	return (
		<div
			className={[
				"rounded-box border bg-base-100 px-4 py-3",
				tone === "warning" ? "border-warning/40" : "border-base-200",
			].join(" ")}
		>
			<div className="text-xs uppercase tracking-widest opacity-60">
				{label}
			</div>
			<div className="mt-1 font-semibold">{value}</div>
		</div>
	);
}

export function IpGeoDbPage() {
	const [adminToken] = useState(() => readAdminToken());
	const toast = useToast();
	const queryClient = useQueryClient();
	const [autoUpdateEnabled, setAutoUpdateEnabled] = useState(false);
	const [updateIntervalDays, setUpdateIntervalDays] = useState("1");

	const geoDbQuery = useQuery({
		queryKey: ["adminIpGeoDb", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminIpGeoDb(adminToken, signal),
		refetchInterval: (query) =>
			query.state.data?.nodes.some((node) => node.running) ? 2000 : false,
	});

	useEffect(() => {
		if (!geoDbQuery.data) return;
		setAutoUpdateEnabled(geoDbQuery.data.settings.auto_update_enabled);
		setUpdateIntervalDays(
			String(geoDbQuery.data.settings.update_interval_days),
		);
	}, [geoDbQuery.data]);

	const saveMutation = useMutation({
		mutationFn: async () => {
			const parsedInterval = Number(updateIntervalDays);
			return patchAdminIpGeoDb(adminToken, {
				auto_update_enabled: autoUpdateEnabled,
				update_interval_days: parsedInterval,
			});
		},
		onSuccess: async () => {
			toast.pushToast({ variant: "success", message: "Saved Geo DB settings" });
			await queryClient.invalidateQueries({
				queryKey: ["adminIpGeoDb", adminToken],
			});
		},
		onError: (error) => {
			toast.pushToast({ variant: "error", message: formatError(error) });
		},
	});

	const manualUpdateMutation = useMutation({
		mutationFn: async () => triggerAdminIpGeoDbUpdate(adminToken),
		onSuccess: async (result) => {
			const acceptedCount = result.nodes.filter(
				(node) => node.status === "accepted",
			).length;
			toast.pushToast({
				variant: result.partial ? "info" : "success",
				message: result.partial
					? `Triggered with partial reachability (${acceptedCount} accepted)`
					: `Triggered update on ${acceptedCount} node(s)`,
			});
			await queryClient.invalidateQueries({
				queryKey: ["adminIpGeoDb", adminToken],
			});
		},
		onError: (error) => {
			toast.pushToast({ variant: "error", message: formatError(error) });
		},
	});

	const isDirty = useMemo(() => {
		if (!geoDbQuery.data) return false;
		return (
			autoUpdateEnabled !== geoDbQuery.data.settings.auto_update_enabled ||
			Number(updateIntervalDays) !==
				geoDbQuery.data.settings.update_interval_days
		);
	}, [autoUpdateEnabled, geoDbQuery.data, updateIntervalDays]);

	const managedCount =
		geoDbQuery.data?.nodes.filter((node) => node.mode === "managed").length ??
		0;
	const overrideCount =
		geoDbQuery.data?.nodes.filter((node) => node.mode === "external_override")
			.length ?? 0;
	const missingCount =
		geoDbQuery.data?.nodes.filter((node) => node.mode === "missing").length ??
		0;
	const anyRunning =
		geoDbQuery.data?.nodes.some((node) => node.running) ?? false;

	const headerActions = (
		<>
			<Button
				variant="secondary"
				size="sm"
				loading={geoDbQuery.isFetching}
				disabled={adminToken.length === 0}
				onClick={() => geoDbQuery.refetch()}
			>
				Refresh
			</Button>
			<Button
				variant="primary"
				size="sm"
				loading={manualUpdateMutation.isPending}
				disabled={adminToken.length === 0 || anyRunning}
				onClick={() => manualUpdateMutation.mutate()}
			>
				Manual update
			</Button>
		</>
	);

	if (adminToken.length === 0) {
		return (
			<div className="space-y-6">
				<PageHeader
					title="IP geolocation"
					description="Manage the cluster-wide DB-IP Lite geolocation database policy."
				/>
				<PageState
					variant="empty"
					title="需要管理员 Token"
					description="请先在 Dashboard 页面设置 admin token，再查看 Geo DB 状态。"
				/>
			</div>
		);
	}

	if (geoDbQuery.isLoading && !geoDbQuery.data) {
		return (
			<div className="space-y-6">
				<PageHeader
					title="IP geolocation"
					description="Manage the cluster-wide DB-IP Lite geolocation database policy."
					actions={headerActions}
				/>
				<PageState
					variant="loading"
					title="Loading Geo DB settings"
					description="Fetching cluster settings and node-local Geo DB runtime state."
				/>
			</div>
		);
	}

	if (geoDbQuery.isError && !geoDbQuery.data) {
		return (
			<div className="space-y-6">
				<PageHeader
					title="IP geolocation"
					description="Manage the cluster-wide DB-IP Lite geolocation database policy."
					actions={headerActions}
				/>
				<PageState
					variant="error"
					title="Failed to load Geo DB settings"
					description={formatError(geoDbQuery.error)}
					action={
						<Button variant="secondary" onClick={() => geoDbQuery.refetch()}>
							Retry
						</Button>
					}
				/>
			</div>
		);
	}

	const data = geoDbQuery.data;
	if (!data) {
		return null;
	}

	return (
		<div className="space-y-6">
			<PageHeader
				title="IP geolocation"
				description="Cluster-wide DB-IP Lite City + ASN MMDB policy, node runtime status, and manual update control."
				actions={headerActions}
				meta={
					<>
						<span className="badge badge-outline">Provider: DB-IP Lite</span>
						{anyRunning ? (
							<span className="badge badge-info">running</span>
						) : null}
					</>
				}
			/>

			{data.partial ? (
				<div className="alert alert-warning py-2 text-sm">
					<div className="space-y-1">
						<div>Node status is partial.</div>
						<div className="font-mono text-xs">
							Unreachable nodes: {data.unreachable_nodes.join(", ")}
						</div>
					</div>
				</div>
			) : null}

			<div className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
				<SummaryChip
					label="auto update"
					value={data.settings.auto_update_enabled ? "Enabled" : "Disabled"}
				/>
				<SummaryChip
					label="interval"
					value={`${data.settings.update_interval_days} day(s)`}
				/>
				<SummaryChip label="managed nodes" value={String(managedCount)} />
				<SummaryChip
					label="override / missing"
					value={`${overrideCount} / ${missingCount}`}
					tone={missingCount > 0 ? "warning" : "neutral"}
				/>
			</div>

			<div className="grid gap-4 xl:grid-cols-[minmax(0,380px)_minmax(0,1fr)]">
				<div className="card bg-base-100 shadow">
					<div className="card-body space-y-4 p-5">
						<div>
							<h2 className="card-title text-base">Update policy</h2>
							<p className="text-sm opacity-70">
								xp manages DB-IP Lite City + ASN MMDB files under
								<code className="font-mono">XP_DATA_DIR/geoip/</code> unless a
								node explicitly overrides the paths via environment variables.
							</p>
						</div>

						<label className="label cursor-pointer justify-start gap-3 rounded-box border border-base-200 px-3 py-3">
							<input
								type="checkbox"
								className="toggle toggle-primary"
								checked={autoUpdateEnabled}
								onChange={(event) => setAutoUpdateEnabled(event.target.checked)}
							/>
							<div>
								<div className="font-medium">Automatic updates</div>
								<div className="text-xs opacity-70">
									Run the managed DB-IP Lite refresh worker on every node.
								</div>
							</div>
						</label>

						<label className="form-control">
							<div className="label">
								<span className="label-text">Update interval (days)</span>
							</div>
							<input
								type="number"
								min={1}
								max={30}
								className="input input-bordered"
								value={updateIntervalDays}
								onChange={(event) => setUpdateIntervalDays(event.target.value)}
							/>
							<div className="label">
								<span className="label-text-alt opacity-70">
									Allowed range: 1-30 days.
								</span>
							</div>
						</label>

						<div className="flex flex-wrap justify-end gap-2">
							<Button
								variant="secondary"
								disabled={!isDirty}
								onClick={() => {
									setAutoUpdateEnabled(data.settings.auto_update_enabled);
									setUpdateIntervalDays(
										String(data.settings.update_interval_days),
									);
								}}
							>
								Reset
							</Button>
							<Button
								variant="primary"
								loading={saveMutation.isPending}
								disabled={!isDirty}
								onClick={() => saveMutation.mutate()}
							>
								Save settings
							</Button>
						</div>
					</div>
				</div>

				<div className="card bg-base-100 shadow">
					<div className="card-body p-0">
						<div className="flex items-center justify-between px-5 pt-5">
							<div>
								<h2 className="card-title text-base">Node runtime</h2>
								<p className="text-sm opacity-70">
									Every node downloads locally; leader only stores the shared
									settings.
								</p>
							</div>
						</div>
						<div className="overflow-x-auto px-5 pb-5 pt-4">
							<table className="table table-zebra">
								<thead>
									<tr>
										<th>Node</th>
										<th>Mode</th>
										<th>Status</th>
										<th>Next</th>
										<th>Last success</th>
										<th>Paths</th>
									</tr>
								</thead>
								<tbody>
									{data.nodes.map((node) => (
										<tr key={node.node.node_id}>
											<td>
												<div className="font-medium">{node.node.node_name}</div>
												<div className="font-mono text-xs opacity-70">
													{node.node.node_id}
												</div>
											</td>
											<td>
												<span className={`badge ${modeBadgeClass(node.mode)}`}>
													{modeLabel(node.mode)}
												</span>
											</td>
											<td>
												<div className="flex flex-col gap-1 text-xs">
													<span
														className={`badge ${node.running ? "badge-info" : "badge-ghost"}`}
													>
														{node.running ? "Running" : "Idle"}
													</span>
													{node.last_error ? (
														<span className="max-w-xs text-error">
															{node.last_error}
														</span>
													) : null}
												</div>
											</td>
											<td className="text-xs">
												{formatDateTime(node.next_scheduled_at)}
											</td>
											<td className="text-xs">
												{formatDateTime(node.last_success_at)}
											</td>
											<td>
												<div className="max-w-md space-y-1 font-mono text-[11px] opacity-80">
													<div>{node.city_db_path || "(empty city path)"}</div>
													<div>{node.asn_db_path || "(empty ASN path)"}</div>
												</div>
											</td>
										</tr>
									))}
								</tbody>
							</table>
						</div>
					</div>
				</div>
			</div>
		</div>
	);
}
