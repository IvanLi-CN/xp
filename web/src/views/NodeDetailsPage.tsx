import { zodResolver } from "@hookform/resolvers/zod";
import { useQuery } from "@tanstack/react-query";
import { Link, useNavigate, useParams } from "@tanstack/react-router";
import { useEffect, useMemo, useState } from "react";
import { useForm } from "react-hook-form";
import { z } from "zod";

import {
	type AdminIpUsageWindow,
	fetchAdminNodeIpUsage,
} from "../api/adminIpUsage";
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
import { IpUsageView } from "../components/IpUsageView";
import { NodeQuotaEditor } from "../components/NodeQuotaEditor";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
import { useToast } from "../components/Toast";
import { readAdminToken } from "../components/auth";
import { alertClass } from "../components/ui-helpers";
import { Badge } from "../components/ui/badge";
import {
	Form,
	FormControl,
	FormDescription,
	FormField,
	FormItem,
	FormLabel,
	FormMessage,
} from "../components/ui/form";
import { Input } from "../components/ui/input";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "../components/ui/select";
import { Tabs, TabsList, TabsTrigger } from "../components/ui/tabs";

function formatErrorMessage(error: unknown): string {
	if (isBackendApiError(error)) {
		const code = error.code ? ` ${error.code}` : "";
		return `${error.status}${code}: ${error.message}`;
	}
	return String(error);
}

function summaryBadgeVariant(
	status: string,
): "success" | "warning" | "destructive" | "ghost" {
	switch (status) {
		case "up":
			return "success";
		case "degraded":
			return "warning";
		case "down":
			return "destructive";
		default:
			return "ghost";
	}
}

function componentBadgeVariant(
	status: string,
): "success" | "destructive" | "ghost" | "outline" {
	switch (status) {
		case "up":
			return "success";
		case "down":
			return "destructive";
		case "disabled":
			return "ghost";
		default:
			return "outline";
	}
}

function eventBadgeVariant(
	kind: NodeRuntimeEvent["kind"],
): "warning" | "info" | "success" | "destructive" | "ghost" {
	switch (kind) {
		case "status_changed":
			return "warning";
		case "restart_requested":
			return "info";
		case "restart_succeeded":
			return "success";
		case "restart_failed":
			return "destructive";
		default:
			return "ghost";
	}
}

function historySlotClass(status: string): string {
	switch (status) {
		case "up":
			return "bg-success";
		case "degraded":
			return "bg-warning";
		case "down":
			return "bg-destructive";
		case "unknown":
			return "bg-info";
		default:
			return "bg-muted";
	}
}

const SLOTS_PER_DAY = 48;
const ACTIVITY_DAYS = 7;

const quotaResetSchema = z
	.object({
		resetPolicy: z.enum(["monthly", "unlimited"]),
		resetDay: z.coerce
			.number({
				invalid_type_error: "Reset day must be an integer between 1 and 31.",
			})
			.int("Reset day must be an integer between 1 and 31."),
		resetTzOffsetMinutes: z
			.string()
			.trim()
			.refine((value) => value === "" || /^-?\d+$/.test(value), {
				message: "tz_offset_minutes must be an integer (or empty).",
			}),
	})
	.superRefine((values, ctx) => {
		if (
			values.resetPolicy === "monthly" &&
			(values.resetDay < 1 || values.resetDay > 31)
		) {
			ctx.addIssue({
				code: z.ZodIssueCode.custom,
				path: ["resetDay"],
				message: "Reset day must be an integer between 1 and 31.",
			});
		}
	});

type QuotaResetFormValues = z.infer<typeof quotaResetSchema>;

type RuntimeActivityRow = {
	key: string;
	label: string;
	sortKey: number;
	slots: Array<NodeRuntimeHistorySlot | null>;
};

type NodeDetailsTab = "runtime" | "metadata" | "quota" | "ipUsage" | "danger";

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
	const [ipUsageWindow, setIpUsageWindow] = useState<AdminIpUsageWindow>("24h");

	const ipUsageQuery = useQuery({
		queryKey: ["adminNodeIpUsage", adminToken, nodeId, ipUsageWindow],
		enabled: adminToken.length > 0 && activeTab === "ipUsage",
		queryFn: ({ signal }) =>
			fetchAdminNodeIpUsage(adminToken, nodeId, ipUsageWindow, signal),
		placeholderData: (previousData) =>
			previousData?.node.node_id === nodeId ? previousData : undefined,
	});

	useEffect(() => {
		if (!nodeId) return;
		setRuntimeLive(null);
		setRuntimeSseError(null);
		setRuntimeSseConnected(false);
		setActiveTab("runtime");
		setIpUsageWindow("24h");
	}, [nodeId]);

	const [saveError, setSaveError] = useState<string | null>(null);
	const [isSaving, setIsSaving] = useState(false);
	const [deleteOpen, setDeleteOpen] = useState(false);
	const [isDeleting, setIsDeleting] = useState(false);

	const quotaForm = useForm<QuotaResetFormValues>({
		resolver: zodResolver(quotaResetSchema),
		defaultValues: {
			resetPolicy: "monthly",
			resetDay: 1,
			resetTzOffsetMinutes: "",
		},
	});

	const resetQuotaForm = quotaForm.reset;

	useEffect(() => {
		if (!nodeQuery.data) return;
		const q = nodeQuery.data.quota_reset;
		resetQuotaForm({
			resetPolicy: q.policy === "monthly" ? "monthly" : "unlimited",
			resetDay: q.policy === "monthly" ? q.day_of_month : 1,
			resetTzOffsetMinutes:
				q.tz_offset_minutes === null || q.tz_offset_minutes === undefined
					? ""
					: String(q.tz_offset_minutes),
		});
		setSaveError(null);
	}, [nodeQuery.data, resetQuotaForm]);

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

	const quotaValues = quotaForm.watch();

	const desiredQuotaReset: NodeQuotaReset = useMemo(() => {
		const raw = quotaValues.resetTzOffsetMinutes.trim();
		const tz = raw.length === 0 ? undefined : Number(raw);
		return quotaValues.resetPolicy === "monthly"
			? {
					policy: "monthly",
					day_of_month: quotaValues.resetDay,
					...(tz === undefined ? {} : { tz_offset_minutes: tz }),
				}
			: {
					policy: "unlimited",
					...(tz === undefined ? {} : { tz_offset_minutes: tz }),
				};
	}, [
		quotaValues.resetDay,
		quotaValues.resetPolicy,
		quotaValues.resetTzOffsetMinutes,
	]);

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
			quotaValues.resetPolicy !== currentPolicy ||
			(quotaValues.resetPolicy === "monthly" &&
				quotaValues.resetDay !== currentDay) ||
			quotaValues.resetTzOffsetMinutes.trim() !== currentTz
		);
	}, [
		nodeQuery.data,
		quotaValues.resetDay,
		quotaValues.resetPolicy,
		quotaValues.resetTzOffsetMinutes,
	]);

	const handleSaveQuotaReset = quotaForm.handleSubmit(async () => {
		if (!nodeQuery.data) return;
		if (!isDirty) {
			pushToast({ variant: "info", message: "No changes to save." });
			return;
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
	});

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

		const runtime = runtimeLive ?? runtimeQuery.data;
		const quotaPolicy = quotaForm.watch("resetPolicy");

		return (
			<div className="space-y-4">
				<Tabs
					value={activeTab}
					onValueChange={(value) => setActiveTab(value as NodeDetailsTab)}
				>
					<div className="overflow-x-auto">
						<TabsList
							className="h-auto min-w-max justify-start gap-1 rounded-2xl border border-border/70 bg-card p-1 shadow-sm"
							aria-label="Node details sections"
						>
							<TabsTrigger
								value="runtime"
								className="whitespace-nowrap"
								onClick={() => setActiveTab("runtime")}
							>
								Service runtime
							</TabsTrigger>
							<TabsTrigger
								value="metadata"
								className="whitespace-nowrap"
								onClick={() => setActiveTab("metadata")}
							>
								Node metadata
							</TabsTrigger>
							<TabsTrigger
								value="quota"
								className="whitespace-nowrap"
								onClick={() => setActiveTab("quota")}
							>
								Quota reset
							</TabsTrigger>
							<TabsTrigger
								value="ipUsage"
								className="whitespace-nowrap"
								onClick={() => setActiveTab("ipUsage")}
							>
								IP usage
							</TabsTrigger>
							<TabsTrigger
								value="danger"
								className="whitespace-nowrap"
								onClick={() => setActiveTab("danger")}
							>
								Danger zone
							</TabsTrigger>
						</TabsList>
					</div>

					{activeTab === "runtime" ? (
						<div className="xp-card">
							<div className="xp-card-body space-y-4">
								<div className="flex items-center justify-between gap-3">
									<div>
										<h2 className="xp-card-title">Service runtime</h2>
										<p className="text-sm text-muted-foreground">
											Live status of xp/xray/cloudflared with 7-day history and
											key events.
										</p>
									</div>
									<div className="flex items-center gap-2">
										{runtime ? (
											<Badge
												variant={summaryBadgeVariant(runtime.summary.status)}
											>
												{runtime.summary.status}
											</Badge>
										) : null}
										<Badge variant={runtimeSseConnected ? "success" : "ghost"}>
											{runtimeSseConnected ? "live" : "polling"}
										</Badge>
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
											<div className={alertClass("warning", "py-2")}>
												<span>Realtime stream degraded: {runtimeSseError}</span>
											</div>
										) : null}

										<div className="grid gap-3 lg:grid-cols-3">
											{runtime.components.map((component) => (
												<div
													key={component.component}
													className="space-y-2 rounded-2xl border border-border/70 bg-muted/35 p-3"
												>
													<div className="flex items-center justify-between gap-2">
														<p className="font-semibold">
															{component.component}
														</p>
														<Badge
															variant={componentBadgeVariant(component.status)}
															size="sm"
														>
															{component.status}
														</Badge>
													</div>
													<div className="space-y-1 font-mono text-xs opacity-80">
														<p>last_ok: {formatTime(component.last_ok_at)}</p>
														<p>
															last_fail: {formatTime(component.last_fail_at)}
														</p>
														<p>
															down_since: {formatTime(component.down_since)}
														</p>
														<p>fails: {component.consecutive_failures}</p>
														<p>recoveries: {component.recoveries_observed}</p>
														<p>
															restart_attempts: {component.restart_attempts}
														</p>
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
												<p className="text-xs uppercase tracking-wide text-muted-foreground">
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
														<span className="size-2 rounded-[2px] bg-destructive" />
														down
													</span>
													<span className="inline-flex items-center gap-1">
														<span className="size-2 rounded-[2px] bg-info" />
														unknown
													</span>
												</div>
											</div>

											<div className="rounded-2xl border border-border/70 bg-muted/35 p-3">
												<div className="overflow-x-auto">
													<div className="min-w-[28rem]">
														<div
															className="mb-1 grid items-center gap-2 text-[10px] text-muted-foreground"
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
															{buildRuntimeActivityRows(
																runtime.recent_slots,
															).map((row) => (
																<div
																	key={row.key}
																	className="grid items-center gap-2"
																	style={{
																		gridTemplateColumns: "4.5rem minmax(0,1fr)",
																	}}
																>
																	<span className="truncate font-mono text-[11px] text-muted-foreground">
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
																						: "bg-muted/60"
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
															))}
														</div>
													</div>
												</div>
											</div>
										</div>

										<div className="space-y-2">
											<div className="flex items-center justify-between gap-2">
												<p className="text-xs uppercase tracking-wide text-muted-foreground">
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
											<div className="max-h-72 overflow-auto rounded-2xl border border-border/70 bg-muted/35">
												<table className="xp-table xp-table-compact">
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
																		<Badge
																			variant={eventBadgeVariant(event.kind)}
																			size="sm"
																		>
																			{event.kind}
																		</Badge>
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
						<div className="xp-card">
							<div className="xp-card-body space-y-3">
								<div>
									<h2 className="xp-card-title">Node metadata</h2>
									<p className="text-sm text-muted-foreground">
										Read-only. Managed via xp-ops config file.
									</p>
								</div>
								<div className="rounded-2xl bg-muted/35 p-4 space-y-2">
									<div className="text-xs uppercase tracking-wide text-muted-foreground">
										Node ID
									</div>
									<div className="font-mono text-sm break-all">{nodeId}</div>
									<div className="grid gap-2 md:grid-cols-2 pt-3">
										<div>
											<div className="text-xs uppercase tracking-wide text-muted-foreground">
												Node name
											</div>
											<div className="font-mono text-sm break-all">
												{nodeQuery.data.node_name || "(empty)"}
											</div>
										</div>
										<div>
											<div className="text-xs uppercase tracking-wide text-muted-foreground">
												Access host
											</div>
											<div className="font-mono text-sm break-all">
												{nodeQuery.data.access_host || "(empty)"}
											</div>
										</div>
										<div className="md:col-span-2">
											<div className="text-xs uppercase tracking-wide text-muted-foreground">
												API base URL
											</div>
											<div className="font-mono text-sm break-all">
												{nodeQuery.data.api_base_url || "(empty)"}
											</div>
										</div>
									</div>
								</div>
								<div className="text-sm text-muted-foreground">
									To change node meta, edit{" "}
									<span className="font-mono">/etc/xp/xp.env</span> and run{" "}
									<span className="font-mono">
										sudo xp-ops xp sync-node-meta
									</span>
									.
								</div>
							</div>
						</div>
					) : null}

					{activeTab === "quota" ? (
						<div className="xp-card">
							<div className="xp-card-body space-y-4">
								<div>
									<h2 className="xp-card-title">Quota reset</h2>
									<p className="text-sm text-muted-foreground">
										Runtime admin setting. Safe to edit via the admin API.
									</p>
								</div>
								<div className="rounded-2xl bg-muted/35 p-4 space-y-1">
									<div className="text-xs uppercase tracking-wide text-muted-foreground">
										Quota budget
									</div>
									<div className="text-sm text-muted-foreground">
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
								<Form {...quotaForm}>
									<form className="space-y-4" onSubmit={handleSaveQuotaReset}>
										<div className="grid gap-4 md:grid-cols-3">
											<FormField
												control={quotaForm.control}
												name="resetPolicy"
												render={({ field }) => (
													<FormItem>
														<FormLabel>Policy</FormLabel>
														<Select
															value={field.value}
															onValueChange={(value) => {
																field.onChange(value);
																if (value !== "monthly") {
																	quotaForm.setValue("resetDay", 1, {
																		shouldDirty: true,
																		shouldValidate: false,
																	});
																	quotaForm.clearErrors("resetDay");
																}
															}}
														>
															<FormControl>
																<SelectTrigger>
																	<SelectValue />
																</SelectTrigger>
															</FormControl>
															<SelectContent>
																<SelectItem value="monthly">monthly</SelectItem>
																<SelectItem value="unlimited">
																	unlimited
																</SelectItem>
															</SelectContent>
														</Select>
														<FormMessage />
													</FormItem>
												)}
											/>
											<FormField
												control={quotaForm.control}
												name="resetDay"
												render={({ field }) => (
													<FormItem>
														<FormLabel>Day of month</FormLabel>
														<FormControl>
															<Input
																{...field}
																type="number"
																min={1}
																max={31}
																step={1}
																disabled={quotaPolicy !== "monthly"}
																onChange={(event) =>
																	field.onChange(event.target.value)
																}
															/>
														</FormControl>
														<FormDescription>
															Used when the policy is monthly.
														</FormDescription>
														<FormMessage />
													</FormItem>
												)}
											/>
											<FormField
												control={quotaForm.control}
												name="resetTzOffsetMinutes"
												render={({ field }) => (
													<FormItem>
														<FormLabel>tz_offset_minutes</FormLabel>
														<FormControl>
															<Input
																{...field}
																type="text"
																placeholder="(empty)"
															/>
														</FormControl>
														<FormDescription>
															Leave empty to follow node-local defaults.
														</FormDescription>
														<FormMessage />
													</FormItem>
												)}
											/>
										</div>

										{saveError ? (
											<p className="text-sm text-destructive">{saveError}</p>
										) : null}

										<div className="flex justify-end gap-2">
											<Button
												variant="secondary"
												type="button"
												loading={nodeQuery.isFetching}
												onClick={() => nodeQuery.refetch()}
											>
												Refresh
											</Button>
											<Button
												type="submit"
												loading={isSaving}
												disabled={!isDirty}
											>
												Save changes
											</Button>
										</div>
									</form>
								</Form>
							</div>
						</div>
					) : null}

					{activeTab === "ipUsage" ? (
						<div className="space-y-4">
							{ipUsageQuery.isLoading && !ipUsageQuery.data ? (
								<PageState
									variant="loading"
									title="Loading IP usage"
									description="Fetching minute-level inbound IP usage for this node."
								/>
							) : null}

							{ipUsageQuery.isError && !ipUsageQuery.data ? (
								<PageState
									variant="error"
									title="Failed to load IP usage"
									description={formatErrorMessage(ipUsageQuery.error)}
									action={
										<Button
											variant="secondary"
											loading={ipUsageQuery.isFetching}
											onClick={() => ipUsageQuery.refetch()}
										>
											Retry
										</Button>
									}
								/>
							) : null}

							{ipUsageQuery.data ? (
								<IpUsageView
									title="IP usage"
									description="Per-minute unique inbound IP counts, occupancy lanes, and aggregated IP rows for this node."
									window={ipUsageWindow}
									geoSource={ipUsageQuery.data.geo_source}
									onWindowChange={setIpUsageWindow}
									report={ipUsageQuery.data}
									isFetching={ipUsageQuery.isFetching}
									emptyTitle="No inbound IP activity"
								/>
							) : null}
						</div>
					) : null}

					{activeTab === "danger" ? (
						<div className="xp-card border border-destructive/30">
							<div className="xp-card-body space-y-4">
								<h2 className="xp-card-title text-destructive">Danger zone</h2>
								<p className="text-sm text-muted-foreground">
									Deleting a node removes it from the cluster membership and
									inventory. This action cannot be undone. Only delete nodes
									that have no endpoints.
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
				</Tabs>

				<ConfirmDialog
					open={deleteOpen}
					title="Delete node?"
					description="This action cannot be undone. The node must have no endpoints."
					onCancel={() => setDeleteOpen(false)}
					footer={
						<div className="flex justify-end gap-2">
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
					<Button asChild variant="ghost" size="sm">
						<Link to="/nodes">Back</Link>
					</Button>
				}
			/>
			{content}
		</div>
	);
}
