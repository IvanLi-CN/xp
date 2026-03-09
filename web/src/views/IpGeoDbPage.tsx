import { zodResolver } from "@hookform/resolvers/zod";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import { useForm } from "react-hook-form";
import { z } from "zod";

import {
	type AdminIpGeoDbNodeStatus,
	fetchAdminIpGeoDb,
	patchAdminIpGeoDb,
	triggerAdminIpGeoDbUpdate,
} from "../api/adminIpGeoDb";
import { isBackendApiError } from "../api/backendError";
import { Button } from "../components/Button";
import { DataTable, TableCell } from "../components/DataTable";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
import { useToast } from "../components/Toast";
import { readAdminToken } from "../components/auth";
import { Badge } from "../components/ui/badge";
import { Checkbox } from "../components/ui/checkbox";
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

const geoDbSettingsSchema = z.object({
	autoUpdateEnabled: z.boolean(),
	updateIntervalDays: z.coerce
		.number()
		.int("Allowed range: 1-30 days.")
		.min(1, "Allowed range: 1-30 days.")
		.max(30, "Allowed range: 1-30 days."),
});

type GeoDbSettingsValues = z.infer<typeof geoDbSettingsSchema>;

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

function modeBadgeVariant(
	mode: AdminIpGeoDbNodeStatus["mode"],
): "success" | "info" | "warning" {
	switch (mode) {
		case "managed":
			return "success";
		case "external_override":
			return "info";
		case "missing":
			return "warning";
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
			className={
				tone === "warning"
					? "xp-panel border-warning/40 px-4 py-3"
					: "xp-panel px-4 py-3"
			}
		>
			<div className="text-xs uppercase tracking-widest text-muted-foreground">
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
	const form = useForm<GeoDbSettingsValues>({
		resolver: zodResolver(geoDbSettingsSchema),
		defaultValues: {
			autoUpdateEnabled: false,
			updateIntervalDays: 1,
		},
	});

	const geoDbQuery = useQuery({
		queryKey: ["adminIpGeoDb", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminIpGeoDb(adminToken, signal),
		refetchInterval: (query) =>
			query.state.data?.nodes.some((node) => node.running) ? 2000 : false,
	});

	useEffect(() => {
		if (!geoDbQuery.data) return;
		form.reset({
			autoUpdateEnabled: geoDbQuery.data.settings.auto_update_enabled,
			updateIntervalDays: geoDbQuery.data.settings.update_interval_days,
		});
	}, [form, geoDbQuery.data]);

	const saveMutation = useMutation({
		mutationFn: async (values: GeoDbSettingsValues) =>
			patchAdminIpGeoDb(adminToken, {
				auto_update_enabled: values.autoUpdateEnabled,
				update_interval_days: values.updateIntervalDays,
			}),
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

	const values = form.watch();
	const isDirty = useMemo(() => {
		if (!geoDbQuery.data) return false;
		return (
			values.autoUpdateEnabled !==
				geoDbQuery.data.settings.auto_update_enabled ||
			Number(values.updateIntervalDays) !==
				geoDbQuery.data.settings.update_interval_days
		);
	}, [geoDbQuery.data, values.autoUpdateEnabled, values.updateIntervalDays]);

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
	if (!data) return null;

	return (
		<div className="space-y-6">
			<PageHeader
				title="IP geolocation"
				description="Cluster-wide DB-IP Lite City + ASN MMDB policy, node runtime status, and manual update control."
				actions={headerActions}
				meta={
					<>
						<Badge variant="outline">Provider: DB-IP Lite</Badge>
						{anyRunning ? <Badge variant="info">running</Badge> : null}
					</>
				}
			/>

			{data.partial ? (
				<div className="rounded-xl border border-warning/30 bg-warning/10 px-4 py-3 text-sm">
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
				<div className="xp-card">
					<div className="xp-card-body px-5 py-5">
						<div>
							<h2 className="xp-card-title text-base">Update policy</h2>
							<p className="text-sm text-muted-foreground">
								xp manages DB-IP Lite City + ASN MMDB files under{" "}
								<code className="font-mono">XP_DATA_DIR/geoip/</code> unless a
								node explicitly overrides the paths via environment variables.
							</p>
						</div>

						<Form {...form}>
							<form
								className="space-y-4"
								onSubmit={form.handleSubmit(async (submittedValues) =>
									saveMutation.mutateAsync(submittedValues),
								)}
							>
								<FormField
									control={form.control}
									name="autoUpdateEnabled"
									render={({ field }) => (
										<FormItem className="rounded-xl border border-border/70 px-4 py-3">
											<div className="flex items-start gap-3">
												<FormControl>
													<Checkbox
														aria-label="Automatic updates"
														checked={field.value}
														onCheckedChange={(checked) =>
															field.onChange(Boolean(checked))
														}
													/>
												</FormControl>
												<div className="space-y-1">
													<FormLabel className="font-medium text-foreground">
														Automatic updates
													</FormLabel>
													<FormDescription className="text-xs">
														Run the managed DB-IP Lite refresh worker on every
														node.
													</FormDescription>
												</div>
											</div>
										</FormItem>
									)}
								/>

								<FormField
									control={form.control}
									name="updateIntervalDays"
									render={({ field }) => (
										<FormItem>
											<FormLabel>Update interval (days)</FormLabel>
											<FormControl>
												<Input
													{...field}
													type="number"
													min={1}
													max={30}
													disabled={saveMutation.isPending}
													onChange={(event) =>
														field.onChange(event.target.value)
													}
												/>
											</FormControl>
											<FormDescription>
												Allowed range: 1-30 days.
											</FormDescription>
											<FormMessage />
										</FormItem>
									)}
								/>

								<div className="flex flex-wrap justify-end gap-2">
									<Button
										variant="secondary"
										type="button"
										disabled={!isDirty}
										onClick={() => {
											form.reset({
												autoUpdateEnabled: data.settings.auto_update_enabled,
												updateIntervalDays: data.settings.update_interval_days,
											});
										}}
									>
										Reset
									</Button>
									<Button
										type="submit"
										loading={saveMutation.isPending}
										disabled={!isDirty}
									>
										Save settings
									</Button>
								</div>
							</form>
						</Form>
					</div>
				</div>

				<div className="xp-card">
					<div className="xp-card-body p-0">
						<div className="px-5 pt-5">
							<h2 className="xp-card-title text-base">Node runtime</h2>
							<p className="text-sm text-muted-foreground">
								Every node downloads locally; leader only stores the shared
								settings.
							</p>
						</div>
						<div className="px-5 pb-5 pt-4">
							<DataTable
								headers={[
									{ key: "node", label: "Node" },
									{ key: "mode", label: "Mode" },
									{ key: "status", label: "Status" },
									{ key: "next", label: "Next" },
									{ key: "lastSuccess", label: "Last success" },
									{ key: "paths", label: "Paths" },
								]}
							>
								{data.nodes.map((node) => (
									<tr key={node.node.node_id}>
										<TableCell>
											<div className="font-medium">{node.node.node_name}</div>
											<div className="font-mono text-xs text-muted-foreground">
												{node.node.node_id}
											</div>
										</TableCell>
										<TableCell>
											<Badge variant={modeBadgeVariant(node.mode)}>
												{modeLabel(node.mode)}
											</Badge>
										</TableCell>
										<TableCell>
											<div className="flex flex-col gap-1 text-xs">
												<Badge variant={node.running ? "info" : "ghost"}>
													{node.running ? "Running" : "Idle"}
												</Badge>
												{node.last_error ? (
													<span className="max-w-xs text-destructive">
														{node.last_error}
													</span>
												) : null}
											</div>
										</TableCell>
										<TableCell className="text-xs">
											{formatDateTime(node.next_scheduled_at)}
										</TableCell>
										<TableCell className="text-xs">
											{formatDateTime(node.last_success_at)}
										</TableCell>
										<TableCell>
											<div className="max-w-md space-y-1 font-mono text-[11px] text-muted-foreground">
												<div>{node.city_db_path || "(empty city path)"}</div>
												<div>{node.asn_db_path || "(empty ASN path)"}</div>
											</div>
										</TableCell>
									</tr>
								))}
							</DataTable>
						</div>
					</div>
				</div>
			</div>
		</div>
	);
}
