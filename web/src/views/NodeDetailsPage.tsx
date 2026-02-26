import { useQuery } from "@tanstack/react-query";
import { Link, useNavigate, useParams } from "@tanstack/react-router";
import { useEffect, useMemo, useState } from "react";

import {
	type AdminNodeRuntimeDetailResponse,
	type NodeRuntimeEvent,
	type NodeRuntimeHistorySlot,
	fetchAdminNodeRuntime,
	startNodeRuntimeEvents,
} from "../api/adminNodeRuntime";
import {
	type AdminNodePatchRequest,
	deleteAdminNode,
	fetchAdminNode,
	patchAdminNode,
} from "../api/adminNodes";
import { isBackendApiError } from "../api/backendError";
import type { NodeQuotaReset } from "../api/quotaReset";
import { Button } from "../components/Button";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { NodeQuotaEditor } from "../components/NodeQuotaEditor";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
import { useToast } from "../components/Toast";
import { useUiPrefs } from "../components/UiPrefs";
import { readAdminToken } from "../components/auth";

function formatErrorMessage(error: unknown): string {
	if (isBackendApiError(error)) {
		const code = error.code ? ` ${error.code}` : "";
		return `${error.status}${code}: ${error.message}`;
	}
	return String(error);
}

function summaryBadgeClass(status: string): string {
	switch (status) {
		case "up":
			return "badge badge-success";
		case "degraded":
			return "badge badge-warning";
		case "down":
			return "badge badge-error";
		default:
			return "badge badge-ghost";
	}
}

function componentBadgeClass(status: string): string {
	switch (status) {
		case "up":
			return "badge badge-success badge-sm";
		case "down":
			return "badge badge-error badge-sm";
		case "disabled":
			return "badge badge-ghost badge-sm";
		default:
			return "badge badge-outline badge-sm";
	}
}

function eventBadgeClass(kind: NodeRuntimeEvent["kind"]): string {
	switch (kind) {
		case "status_changed":
			return "badge badge-warning badge-sm";
		case "restart_requested":
			return "badge badge-info badge-sm";
		case "restart_succeeded":
			return "badge badge-success badge-sm";
		case "restart_failed":
			return "badge badge-error badge-sm";
		default:
			return "badge badge-ghost badge-sm";
	}
}

function historySlotClass(status: string): string {
	switch (status) {
		case "up":
			return "bg-success";
		case "degraded":
			return "bg-warning";
		case "down":
			return "bg-error";
		case "unknown":
			return "bg-info";
		default:
			return "bg-base-300";
	}
}

const SLOTS_PER_DAY = 48;
const ACTIVITY_DAYS = 7;

type RuntimeActivityRow = {
	key: string;
	label: string;
	sortKey: number;
	slots: Array<NodeRuntimeHistorySlot | null>;
};

type NodeDetailsTab = "runtime" | "metadata" | "quota" | "danger";

function buildRuntimeActivityRows(
	recentSlots: NodeRuntimeHistorySlot[],
): RuntimeActivityRow[] {
	const byDay = new Map<string, RuntimeActivityRow>();

	for (const slot of recentSlots) {
		const at = new Date(slot.slot_start);
		if (Number.isNaN(at.getTime())) continue;

		const dayStart = new Date(at.getFullYear(), at.getMonth(), at.getDate());
		const month = String(dayStart.getMonth() + 1).padStart(2, "0");
		const day = String(dayStart.getDate()).padStart(2, "0");
		const key = `${dayStart.getFullYear()}-${month}-${day}`;

		let row = byDay.get(key);
		if (!row) {
			row = {
				key,
				sortKey: dayStart.getTime(),
				label: dayStart.toLocaleDateString(undefined, {
					month: "numeric",
					day: "numeric",
					weekday: "short",
				}),
				slots: new Array(SLOTS_PER_DAY).fill(null),
			};
			byDay.set(key, row);
		}

		const slotIndex = at.getHours() * 2 + (at.getMinutes() >= 30 ? 1 : 0);
		if (slotIndex >= 0 && slotIndex < SLOTS_PER_DAY) {
			row.slots[slotIndex] = slot;
		}
	}

	const rows = Array.from(byDay.values()).sort((a, b) => a.sortKey - b.sortKey);
	if (rows.length > 0) {
		return rows.slice(-ACTIVITY_DAYS);
	}

	const fallbackRows: RuntimeActivityRow[] = [];
	const fallbackSlots = recentSlots.slice(-SLOTS_PER_DAY * ACTIVITY_DAYS);
	for (let dayIndex = 0; dayIndex < ACTIVITY_DAYS; dayIndex += 1) {
		const sliceStart = dayIndex * SLOTS_PER_DAY;
		const daySlots = fallbackSlots.slice(
			sliceStart,
			sliceStart + SLOTS_PER_DAY,
		);
		if (daySlots.length === 0) continue;
		fallbackRows.push({
			key: `fallback-${dayIndex}`,
			label: `day ${dayIndex + 1}`,
			sortKey: dayIndex,
			slots: [
				...daySlots,
				...new Array(Math.max(0, SLOTS_PER_DAY - daySlots.length)).fill(null),
			],
		});
	}

	return fallbackRows;
}

function formatTime(value: string | null | undefined): string {
	if (!value) return "-";
	const dt = new Date(value);
	if (Number.isNaN(dt.getTime())) return value;
	return dt.toLocaleString();
}

export function NodeDetailsPage() {
	const { nodeId } = useParams({ from: "/app/nodes/$nodeId" });
	const [adminToken] = useState(() => readAdminToken());
	const { pushToast } = useToast();
	const prefs = useUiPrefs();
	const navigate = useNavigate();

	const nodeQuery = useQuery({
		queryKey: ["adminNode", adminToken, nodeId],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminNode(adminToken, nodeId, signal),
	});

	const runtimeQuery = useQuery({
		queryKey: ["adminNodeRuntime", adminToken, nodeId],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) =>
			fetchAdminNodeRuntime(adminToken, nodeId, { eventsLimit: 200, signal }),
	});

	const [runtimeLive, setRuntimeLive] =
		useState<AdminNodeRuntimeDetailResponse | null>(null);
	const [runtimeSseConnected, setRuntimeSseConnected] = useState(false);
	const [runtimeSseError, setRuntimeSseError] = useState<string | null>(null);
	const [activeTab, setActiveTab] = useState<NodeDetailsTab>("runtime");

	useEffect(() => {
		if (!nodeId) return;
		setRuntimeLive(null);
		setRuntimeSseError(null);
		setRuntimeSseConnected(false);
		setActiveTab("runtime");
	}, [nodeId]);

	const [resetPolicy, setResetPolicy] = useState<"monthly" | "unlimited">(
		"monthly",
	);
	const [resetDay, setResetDay] = useState(1);
	const [resetTzOffsetMinutes, setResetTzOffsetMinutes] = useState<string>("");
	const [saveError, setSaveError] = useState<string | null>(null);
	const [isSaving, setIsSaving] = useState(false);
	const [deleteOpen, setDeleteOpen] = useState(false);
	const [isDeleting, setIsDeleting] = useState(false);

	useEffect(() => {
		if (nodeQuery.data) {
			const q = nodeQuery.data.quota_reset;
			if (q.policy === "monthly") {
				setResetPolicy("monthly");
				setResetDay(q.day_of_month);
			} else {
				setResetPolicy("unlimited");
				setResetDay(1);
			}
			const tz =
				q.tz_offset_minutes === null || q.tz_offset_minutes === undefined
					? ""
					: String(q.tz_offset_minutes);
			setResetTzOffsetMinutes(tz);
			setSaveError(null);
		}
	}, [nodeQuery.data]);

	useEffect(() => {
		if (runtimeQuery.data) {
			setRuntimeLive(runtimeQuery.data);
		}
	}, [runtimeQuery.data]);

	useEffect(() => {
		if (adminToken.length === 0) return;

		let unmounted = false;
		const stream = startNodeRuntimeEvents({
			adminToken,
			nodeId,
			onOpen: () => {
				if (unmounted) return;
				setRuntimeSseConnected(true);
				setRuntimeSseError(null);
			},
			onClose: () => {
				if (unmounted) return;
				setRuntimeSseConnected(false);
			},
			onError: (error) => {
				if (unmounted) return;
				setRuntimeSseConnected(false);
				setRuntimeSseError(formatErrorMessage(error));
			},
			onMessage: (message) => {
				if (unmounted) return;
				if (message.type === "snapshot") {
					setRuntimeLive((prev) => {
						if (!prev) return prev;
						return {
							...prev,
							summary: message.data.summary,
							components: message.data.components,
							recent_slots: message.data.recent_slots,
							events: message.data.events,
						};
					});
					return;
				}
				if (message.type === "event") {
					setRuntimeLive((prev) => {
						if (!prev) return prev;
						return {
							...prev,
							events: [message.data, ...prev.events].slice(0, 200),
						};
					});
					void runtimeQuery.refetch();
					return;
				}
				if (message.type === "node_error") {
					setRuntimeSseError(message.data.error);
				}
				if (message.type === "lagged") {
					setRuntimeSseError(
						`SSE lagged: missed ${message.data.missed} events.`,
					);
					void runtimeQuery.refetch();
				}
			},
		});

		return () => {
			unmounted = true;
			stream.close();
		};
	}, [adminToken, nodeId, runtimeQuery.refetch]);

	useEffect(() => {
		if (adminToken.length === 0 || runtimeSseConnected) return;
		const timer = window.setInterval(() => {
			void runtimeQuery.refetch();
		}, 10000);
		return () => window.clearInterval(timer);
	}, [adminToken, runtimeSseConnected, runtimeQuery.refetch]);

	const desiredQuotaReset: NodeQuotaReset = useMemo(() => {
		const raw = resetTzOffsetMinutes.trim();
		const tz =
			raw.length === 0
				? undefined
				: Number.isFinite(Number(raw))
					? Number(raw)
					: undefined;
		return resetPolicy === "monthly"
			? {
					policy: "monthly",
					day_of_month: resetDay,
					...(tz === undefined ? {} : { tz_offset_minutes: tz }),
				}
			: {
					policy: "unlimited",
					...(tz === undefined ? {} : { tz_offset_minutes: tz }),
				};
	}, [resetDay, resetPolicy, resetTzOffsetMinutes]);

	const isDirty = useMemo(() => {
		if (!nodeQuery.data) return false;
		const q = nodeQuery.data.quota_reset;
		const currentPolicy = q.policy;
		const currentDay = currentPolicy === "monthly" ? q.day_of_month : 1;
		const currentTz =
			q.tz_offset_minutes === null || q.tz_offset_minutes === undefined
				? ""
				: String(q.tz_offset_minutes);
		return (
			resetPolicy !== currentPolicy ||
			(resetPolicy === "monthly" && resetDay !== currentDay) ||
			resetTzOffsetMinutes.trim() !== currentTz
		);
	}, [nodeQuery.data, resetDay, resetPolicy, resetTzOffsetMinutes]);

	const handleSaveQuotaReset = async () => {
		if (!nodeQuery.data) return;
		if (!isDirty) {
			pushToast({ variant: "info", message: "No changes to save." });
			return;
		}

		if (
			resetPolicy === "monthly" &&
			(!Number.isInteger(resetDay) || resetDay < 1 || resetDay > 31)
		) {
			setSaveError("Reset day must be an integer between 1 and 31.");
			return;
		}

		const tzRaw = resetTzOffsetMinutes.trim();
		if (tzRaw.length > 0) {
			const tz = Number(tzRaw);
			if (!Number.isFinite(tz) || !Number.isInteger(tz)) {
				setSaveError("tz_offset_minutes must be an integer (or empty).");
				return;
			}
		}

		setIsSaving(true);
		setSaveError(null);

		const payload: AdminNodePatchRequest = {
			quota_reset: desiredQuotaReset,
		};

		try {
			await patchAdminNode(adminToken, nodeId, payload);
			pushToast({ variant: "success", message: "Node updated." });
			await nodeQuery.refetch();
		} catch (error) {
			const message = formatErrorMessage(error);
			setSaveError(message);
			pushToast({
				variant: "error",
				message: "Failed to update node.",
			});
		} finally {
			setIsSaving(false);
		}
	};

	const content = (() => {
		if (adminToken.length === 0) {
			return (
				<PageState
					variant="empty"
					title="Admin token required"
					description="Please provide an admin token to load node details."
				/>
			);
		}

		if (nodeQuery.isLoading) {
			return (
				<PageState
					variant="loading"
					title="Loading node"
					description="Fetching node metadata."
				/>
			);
		}

		if (nodeQuery.isError) {
			return (
				<PageState
					variant="error"
					title="Failed to load node"
					description={formatErrorMessage(nodeQuery.error)}
					action={
						<Button
							variant="secondary"
							loading={nodeQuery.isFetching}
							onClick={() => nodeQuery.refetch()}
						>
							Retry
						</Button>
					}
				/>
			);
		}

		if (!nodeQuery.data) {
			return (
				<PageState
					variant="empty"
					title="Node not found"
					description="No node data is available for this ID."
				/>
			);
		}

		const inputClass =
			prefs.density === "compact"
				? "input input-bordered input-sm"
				: "input input-bordered";
		const selectClass =
			prefs.density === "compact"
				? "select select-bordered select-sm"
				: "select select-bordered";
		const runtime = runtimeLive ?? runtimeQuery.data;

		return (
			<div className="space-y-4">
				<div className="overflow-x-auto">
					<div
						className="inline-flex min-w-max items-center gap-1 rounded-box border border-base-300 bg-base-100 p-1 shadow-sm"
						role="tablist"
						aria-label="Node details sections"
					>
						<button
							type="button"
							role="tab"
							aria-selected={activeTab === "runtime"}
							className={`btn btn-sm whitespace-nowrap ${
								activeTab === "runtime" ? "btn-primary" : "btn-ghost"
							}`}
							onClick={() => setActiveTab("runtime")}
						>
							Service runtime
						</button>
						<button
							type="button"
							role="tab"
							aria-selected={activeTab === "metadata"}
							className={`btn btn-sm whitespace-nowrap ${
								activeTab === "metadata" ? "btn-primary" : "btn-ghost"
							}`}
							onClick={() => setActiveTab("metadata")}
						>
							Node metadata
						</button>
						<button
							type="button"
							role="tab"
							aria-selected={activeTab === "quota"}
							className={`btn btn-sm whitespace-nowrap ${
								activeTab === "quota" ? "btn-primary" : "btn-ghost"
							}`}
							onClick={() => setActiveTab("quota")}
						>
							Quota reset
						</button>
						<button
							type="button"
							role="tab"
							aria-selected={activeTab === "danger"}
							className={`btn btn-sm whitespace-nowrap ${
								activeTab === "danger" ? "btn-primary" : "btn-ghost"
							}`}
							onClick={() => setActiveTab("danger")}
						>
							Danger zone
						</button>
					</div>
				</div>

				{activeTab === "runtime" ? (
					<div className="card bg-base-100 shadow">
						<div className="card-body space-y-4">
							<div className="flex items-center justify-between gap-3">
								<div>
									<h2 className="card-title">Service runtime</h2>
									<p className="text-sm opacity-70">
										Live status of xp/xray/cloudflared with 7-day history and
										key events.
									</p>
								</div>
								<div className="flex items-center gap-2">
									{runtime ? (
										<span className={summaryBadgeClass(runtime.summary.status)}>
											{runtime.summary.status}
										</span>
									) : null}
									<span
										className={
											runtimeSseConnected
												? "badge badge-success badge-outline"
												: "badge badge-ghost"
										}
									>
										{runtimeSseConnected ? "live" : "polling"}
									</span>
								</div>
							</div>

							{runtimeQuery.isLoading && !runtime ? (
								<PageState
									variant="loading"
									title="Loading runtime"
									description="Fetching service runtime details."
								/>
							) : null}

							{runtimeQuery.isError && !runtime ? (
								<PageState
									variant="error"
									title="Failed to load runtime"
									description={formatErrorMessage(runtimeQuery.error)}
									action={
										<Button
											variant="secondary"
											loading={runtimeQuery.isFetching}
											onClick={() => runtimeQuery.refetch()}
										>
											Retry
										</Button>
									}
								/>
							) : null}

							{runtime ? (
								<>
									{runtimeSseError ? (
										<div className="alert alert-warning py-2 text-sm">
											<span>Realtime stream degraded: {runtimeSseError}</span>
										</div>
									) : null}

									<div className="grid gap-3 lg:grid-cols-3">
										{runtime.components.map((component) => (
											<div
												key={component.component}
												className="rounded-box border border-base-300 bg-base-200 p-3 space-y-2"
											>
												<div className="flex items-center justify-between gap-2">
													<p className="font-semibold">{component.component}</p>
													<span
														className={componentBadgeClass(component.status)}
													>
														{component.status}
													</span>
												</div>
												<div className="space-y-1 text-xs font-mono opacity-80">
													<p>last_ok: {formatTime(component.last_ok_at)}</p>
													<p>last_fail: {formatTime(component.last_fail_at)}</p>
													<p>down_since: {formatTime(component.down_since)}</p>
													<p>fails: {component.consecutive_failures}</p>
													<p>recoveries: {component.recoveries_observed}</p>
													<p>restart_attempts: {component.restart_attempts}</p>
													<p>
														last_restart:{" "}
														{formatTime(component.last_restart_at)}
													</p>
												</div>
											</div>
										))}
									</div>

									<div>
										<div className="mb-2 flex flex-wrap items-center justify-between gap-2">
											<p className="text-xs uppercase tracking-wide opacity-60">
												7-day activity (30-minute slots)
											</p>
											<div className="flex items-center gap-3 text-[11px] opacity-70">
												<span className="inline-flex items-center gap-1">
													<span className="size-2 rounded-[2px] bg-success" />
													up
												</span>
												<span className="inline-flex items-center gap-1">
													<span className="size-2 rounded-[2px] bg-warning" />
													degraded
												</span>
												<span className="inline-flex items-center gap-1">
													<span className="size-2 rounded-[2px] bg-error" />
													down
												</span>
												<span className="inline-flex items-center gap-1">
													<span className="size-2 rounded-[2px] bg-info" />
													unknown
												</span>
											</div>
										</div>

										<div className="rounded-box border border-base-300 bg-base-200 p-3">
											<div className="overflow-x-auto">
												<div className="min-w-[28rem]">
													<div
														className="mb-1 grid items-center gap-2 text-[10px] opacity-60"
														style={{
															gridTemplateColumns: "4.5rem minmax(0,1fr)",
														}}
													>
														<span />
														<div className="flex items-center justify-between">
															<span>00:00</span>
															<span>06:00</span>
															<span>12:00</span>
															<span>18:00</span>
															<span>24:00</span>
														</div>
													</div>

													<div className="space-y-1.5">
														{buildRuntimeActivityRows(runtime.recent_slots).map(
															(row) => (
																<div
																	key={row.key}
																	className="grid items-center gap-2"
																	style={{
																		gridTemplateColumns: "4.5rem minmax(0,1fr)",
																	}}
																>
																	<span className="truncate text-[11px] font-mono opacity-70">
																		{row.label}
																	</span>
																	<div
																		className="grid h-3 min-w-0 gap-px"
																		style={{
																			gridTemplateColumns:
																				"repeat(48, minmax(0, 1fr))",
																		}}
																	>
																		{row.slots.map((slot, index) => (
																			<div
																				key={`${row.key}-${index}`}
																				className={`rounded-[1px] ${
																					slot
																						? historySlotClass(slot.status)
																						: "bg-base-300/40"
																				}`}
																				title={
																					slot
																						? `${slot.slot_start} • ${slot.status}`
																						: undefined
																				}
																			/>
																		))}
																	</div>
																</div>
															),
														)}
													</div>
												</div>
											</div>
										</div>
									</div>

									<div className="space-y-2">
										<div className="flex items-center justify-between gap-2">
											<p className="text-xs uppercase tracking-wide opacity-60">
												Key events
											</p>
											<Button
												variant="secondary"
												loading={runtimeQuery.isFetching}
												onClick={() => runtimeQuery.refetch()}
											>
												Refresh runtime
											</Button>
										</div>
										<div className="rounded-box border border-base-300 bg-base-200 max-h-72 overflow-auto">
											<table className="table table-sm">
												<thead>
													<tr>
														<th>Time</th>
														<th>Component</th>
														<th>Kind</th>
														<th>Message</th>
													</tr>
												</thead>
												<tbody>
													{runtime.events.length === 0 ? (
														<tr>
															<td colSpan={4} className="opacity-60">
																No runtime events in window.
															</td>
														</tr>
													) : (
														runtime.events.map((event) => (
															<tr key={event.event_id}>
																<td className="font-mono text-xs">
																	{formatTime(event.occurred_at)}
																</td>
																<td className="font-mono text-xs">
																	{event.component}
																</td>
																<td>
																	<span className={eventBadgeClass(event.kind)}>
																		{event.kind}
																	</span>
																</td>
																<td className="text-xs">{event.message}</td>
															</tr>
														))
													)}
												</tbody>
											</table>
										</div>
									</div>
								</>
							) : null}
						</div>
					</div>
				) : null}

				{activeTab === "metadata" ? (
					<div className="card bg-base-100 shadow">
						<div className="card-body space-y-3">
							<div>
								<h2 className="card-title">Node metadata</h2>
								<p className="text-sm opacity-70">
									Read-only. Managed via xp-ops config file.
								</p>
							</div>
							<div className="rounded-box bg-base-200 p-4 space-y-2">
								<div className="text-xs uppercase tracking-wide opacity-60">
									Node ID
								</div>
								<div className="font-mono text-sm break-all">{nodeId}</div>
								<div className="grid gap-2 md:grid-cols-2 pt-3">
									<div>
										<div className="text-xs uppercase tracking-wide opacity-60">
											Node name
										</div>
										<div className="font-mono text-sm break-all">
											{nodeQuery.data.node_name || "(empty)"}
										</div>
									</div>
									<div>
										<div className="text-xs uppercase tracking-wide opacity-60">
											Access host
										</div>
										<div className="font-mono text-sm break-all">
											{nodeQuery.data.access_host || "(empty)"}
										</div>
									</div>
									<div className="md:col-span-2">
										<div className="text-xs uppercase tracking-wide opacity-60">
											API base URL
										</div>
										<div className="font-mono text-sm break-all">
											{nodeQuery.data.api_base_url || "(empty)"}
										</div>
									</div>
								</div>
							</div>
							<div className="text-sm opacity-70">
								To change node meta, edit{" "}
								<span className="font-mono">/etc/xp/xp.env</span> and run{" "}
								<span className="font-mono">sudo xp-ops xp sync-node-meta</span>
								.
							</div>
						</div>
					</div>
				) : null}

				{activeTab === "quota" ? (
					<div className="card bg-base-100 shadow">
						<div className="card-body space-y-4">
							<div>
								<h2 className="card-title">Quota reset</h2>
								<p className="text-sm opacity-70">
									Runtime admin setting. Safe to edit via the admin API.
								</p>
							</div>
							<div className="rounded-box bg-base-200 p-4 space-y-1">
								<div className="text-xs uppercase tracking-wide opacity-60">
									Quota budget
								</div>
								<div className="text-sm opacity-70">
									Total bytes per cycle for this node. Set to{" "}
									<span className="font-mono">0</span> to disable shared quota
									enforcement (unlimited).
								</div>
								<NodeQuotaEditor
									value={nodeQuery.data.quota_limit_bytes}
									disabled={isSaving}
									onApply={async (nextBytes: number) => {
										try {
											await patchAdminNode(adminToken, nodeId, {
												quota_limit_bytes: nextBytes,
											});
											pushToast({
												variant: "success",
												message: "Node quota budget updated.",
											});
											await nodeQuery.refetch();
										} catch (error) {
											const message = formatErrorMessage(error);
											pushToast({
												variant: "error",
												message: `Failed to update node quota budget: ${message}`,
											});
											throw new Error(message);
										}
									}}
								/>
							</div>
							<div className="grid gap-4 md:grid-cols-3">
								<label className="form-control">
									<div className="label">
										<span className="label-text">Policy</span>
									</div>
									<select
										className={selectClass}
										value={resetPolicy}
										onChange={(e) =>
											setResetPolicy(e.target.value as "monthly" | "unlimited")
										}
									>
										<option value="monthly">monthly</option>
										<option value="unlimited">unlimited</option>
									</select>
								</label>
								<label className="form-control">
									<div className="label">
										<span className="label-text">Day of month</span>
									</div>
									<input
										className={inputClass}
										type="number"
										min={1}
										max={31}
										step={1}
										disabled={resetPolicy !== "monthly"}
										value={resetDay}
										onChange={(e) => setResetDay(Number(e.target.value))}
									/>
								</label>
								<label className="form-control">
									<div className="label">
										<span className="label-text">tz_offset_minutes</span>
									</div>
									<input
										className={inputClass}
										type="text"
										value={resetTzOffsetMinutes}
										onChange={(e) => setResetTzOffsetMinutes(e.target.value)}
										placeholder="(empty)"
									/>
								</label>
							</div>

							{saveError ? (
								<p className="text-sm text-error">{saveError}</p>
							) : null}

							<div className="flex justify-end gap-2">
								<Button
									variant="secondary"
									loading={nodeQuery.isFetching}
									onClick={() => nodeQuery.refetch()}
								>
									Refresh
								</Button>
								<Button
									variant="primary"
									loading={isSaving}
									disabled={!isDirty}
									onClick={handleSaveQuotaReset}
								>
									Save changes
								</Button>
							</div>
						</div>
					</div>
				) : null}

				{activeTab === "danger" ? (
					<div className="card bg-base-100 shadow border border-error/30">
						<div className="card-body space-y-4">
							<h2 className="card-title text-error">Danger zone</h2>
							<p className="text-sm opacity-70">
								Deleting a node removes it from the cluster membership and
								inventory. This action cannot be undone. Only delete nodes that
								have no endpoints.
							</p>
							<div>
								<Button
									variant="danger"
									onClick={() => setDeleteOpen(true)}
									disabled={isDeleting}
								>
									Delete node
								</Button>
							</div>
						</div>
					</div>
				) : null}

				<ConfirmDialog
					open={deleteOpen}
					title="Delete node?"
					description="This action cannot be undone. The node must have no endpoints."
					onCancel={() => setDeleteOpen(false)}
					footer={
						<div className="modal-action">
							<Button
								variant="secondary"
								onClick={() => setDeleteOpen(false)}
								disabled={isDeleting}
							>
								Cancel
							</Button>
							<Button
								variant="danger"
								loading={isDeleting}
								onClick={async () => {
									setIsDeleting(true);
									try {
										await deleteAdminNode(adminToken, nodeId);
										pushToast({
											variant: "success",
											message: "Node deleted.",
										});
										navigate({ to: "/nodes" });
									} catch (error) {
										pushToast({
											variant: "error",
											message: formatErrorMessage(error),
										});
									} finally {
										setIsDeleting(false);
										setDeleteOpen(false);
									}
								}}
							>
								Delete
							</Button>
						</div>
					}
				/>
			</div>
		);
	})();

	return (
		<div className="space-y-6">
			<PageHeader
				title="Node details"
				description="Manage node metadata and routing configuration."
				actions={
					<Link to="/nodes" className="btn btn-ghost btn-sm">
						Back
					</Link>
				}
			/>
			{content}
		</div>
	);
}
