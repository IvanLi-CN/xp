import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

import { fetchAdminConfig, putMihomoResourcePolicy } from "../api/adminConfig";
import { isBackendApiError } from "../api/backendError";
import { fetchClusterInfo } from "../api/clusterInfo";
import { fetchHealth } from "../api/health";
import { Button } from "../components/Button";
import { CopyButton } from "../components/CopyButton";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
import { ReadStateBanner } from "../components/ReadStateBanner";
import { useToast } from "../components/Toast";
import { readAdminToken } from "../components/auth";
import { Checkbox } from "../components/ui/checkbox";
import { useAppRuntime } from "../offline/appRuntime";
import {
	formatSyncTimestamp,
	hasQueryData,
	latestQueryDataUpdatedAt,
	queryIsOfflineBlocked,
} from "../offline/queryReadState";

function formatErrorMessage(error: unknown): string {
	if (isBackendApiError(error)) {
		const code = error.code ? ` ${error.code}` : "";
		return `${error.status}${code}: ${error.message}`;
	}
	return String(error);
}

function displayValue(value: string): string {
	return value.trim().length === 0 ? "(empty)" : value;
}

function formatTimestamp(value: number): string {
	if (!Number.isFinite(value) || value <= 0) return "(empty)";
	const date = new Date(value);
	const yyyy = String(date.getFullYear()).padStart(4, "0");
	const mm = String(date.getMonth() + 1).padStart(2, "0");
	const dd = String(date.getDate()).padStart(2, "0");
	const hh = String(date.getHours()).padStart(2, "0");
	const min = String(date.getMinutes()).padStart(2, "0");
	return `${yyyy}-${mm}-${dd} ${hh}:${min}`;
}

function shortId(value: string, prefixLength = 3): string {
	if (value.length <= prefixLength) return value;
	return `${value.slice(0, prefixLength)}...`;
}

type SummaryChipProps = {
	label: string;
	value: string;
	tone?: "neutral" | "warning";
};

function SummaryChip({ label, value, tone = "neutral" }: SummaryChipProps) {
	return (
		<div
			className={[
				"rounded-2xl border bg-card px-4 py-3 shadow-sm",
				tone === "warning" ? "border-warning/40" : "border-border/70",
			].join(" ")}
		>
			<div className="text-xs uppercase tracking-widest text-muted-foreground">
				{label}
			</div>
			<div className="mt-1 font-semibold text-foreground">{value}</div>
		</div>
	);
}

type FieldBlockProps = {
	label: string;
	value: string;
	copyText?: string;
};

function FieldBlock({ label, value, copyText }: FieldBlockProps) {
	const text = displayValue(value);

	return (
		<div className="rounded-2xl border border-border/70 bg-card px-3 py-3 shadow-sm">
			<div className="flex items-start justify-between gap-3">
				<div className="min-w-0 flex-1">
					<div className="text-xs uppercase tracking-widest text-muted-foreground">
						{label}
					</div>
					<div
						className="mt-1 overflow-x-auto whitespace-nowrap font-mono text-sm"
						title={value}
					>
						{text}
					</div>
				</div>
				{copyText ? (
					<CopyButton
						text={copyText}
						label="copy"
						copiedLabel="copied"
						errorLabel="error"
						variant="secondary"
						size="sm"
						className="h-7 px-2 text-xs uppercase tracking-wide"
					/>
				) : null}
			</div>
		</div>
	);
}

export function ServiceConfigPage() {
	const [adminToken] = useState(() => readAdminToken());
	const toast = useToast();
	const runtime = useAppRuntime();
	const queryClient = useQueryClient();

	const health = useQuery({
		queryKey: ["health"],
		queryFn: ({ signal }) => fetchHealth(signal),
	});

	const clusterInfo = useQuery({
		queryKey: ["clusterInfo"],
		queryFn: ({ signal }) => fetchClusterInfo(signal),
	});

	const configQuery = useQuery({
		queryKey: ["adminConfig", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminConfig(adminToken, signal),
	});

	const privateTargetPolicyMutation = useMutation({
		mutationFn: (allowPrivateTargets: boolean) =>
			putMihomoResourcePolicy(adminToken, allowPrivateTargets),
		onSuccess: (result) => {
			queryClient.setQueryData(
				["adminConfig", adminToken],
				(previous: typeof configQuery.data) =>
					previous
						? {
								...previous,
								mihomo_resource_allow_private_targets:
									result.mihomo_resource_allow_private_targets,
							}
						: previous,
			);
			toast.pushToast({
				variant: "success",
				message: "Mihomo mirror policy updated.",
			});
		},
		onError: (error) => {
			toast.pushToast({
				variant: "error",
				message: `Failed to update Mihomo mirror policy: ${formatErrorMessage(error)}`,
			});
		},
	});

	const headerActions = (
		<>
			<Button
				variant="secondary"
				size="sm"
				disabled={!configQuery.data}
				onClick={async () => {
					if (!configQuery.data) return;
					const payload = JSON.stringify(configQuery.data, null, 2);
					try {
						await navigator.clipboard.writeText(payload);
						toast.pushToast({ variant: "success", message: "Copied JSON" });
					} catch {
						toast.pushToast({ variant: "error", message: "Copy failed" });
					}
				}}
			>
				Copy JSON
			</Button>
			<Button
				variant="primary"
				size="sm"
				loading={configQuery.isFetching}
				disabled={adminToken.length === 0 || runtime.isReadOnly}
				onClick={() => configQuery.refetch()}
			>
				Refresh
			</Button>
		</>
	);

	const content = (() => {
		if (adminToken.length === 0) {
			return (
				<PageState
					variant="empty"
					title="需要管理员 Token"
					description="请先在 Dashboard 页面设置 admin token，再查看服务配置。"
				/>
			);
		}

		if (configQuery.isLoading && !hasQueryData(configQuery)) {
			return (
				<PageState
					variant="loading"
					title="正在加载服务配置"
					description="正在从控制面拉取当前进程的生效配置。"
				/>
			);
		}

		if (
			!hasQueryData(configQuery) &&
			queryIsOfflineBlocked(configQuery, runtime.isOnline)
		) {
			return (
				<PageState
					variant="offline"
					title="Offline cache unavailable"
					description={
						"Open Service config once while online to keep the latest " +
						"effective config available offline."
					}
				/>
			);
		}

		if (configQuery.isError && !hasQueryData(configQuery)) {
			return (
				<PageState
					variant="error"
					title="加载失败"
					description={formatErrorMessage(configQuery.error)}
					error={configQuery.error}
					action={
						<Button
							variant="secondary"
							loading={configQuery.isFetching}
							onClick={() => configQuery.refetch()}
						>
							Retry
						</Button>
					}
				/>
			);
		}

		const data = configQuery.data;
		if (!data) {
			return (
				<PageState
					variant="empty"
					title="没有可展示的数据"
					description="当前没有服务配置数据。"
				/>
			);
		}

		const healthOk = health.isSuccess && health.data?.status === "ok";
		const role = clusterInfo.isSuccess ? clusterInfo.data.role : null;
		const statusValue = `${healthOk ? "Healthy" : "Unhealthy"}${
			role ? ` · ${role}` : ""
		}`;
		const nodeValue = `${
			data.node_name.trim().length > 0 ? data.node_name : "(empty)"
		}${clusterInfo.isSuccess ? ` (${shortId(clusterInfo.data.node_id)})` : ""}`;
		return (
			<div className="space-y-4">
				<div className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
					<SummaryChip label="status" value={statusValue} />
					<SummaryChip label="node" value={nodeValue} />
					<SummaryChip
						label="last refresh"
						value={formatTimestamp(configQuery.dataUpdatedAt)}
					/>
					<SummaryChip
						label="access host"
						value={displayValue(data.access_host)}
					/>
					<SummaryChip label="mihomo" value="provider-only" />
				</div>

				<div className="grid gap-4 lg:grid-cols-2">
					<div className="xp-card">
						<div className="xp-card-body space-y-3">
							<div>
								<h2 className="text-base font-semibold">Network</h2>
								<p className="text-sm text-muted-foreground">
									控制面监听与对外 API 地址。
								</p>
							</div>
							<div className="space-y-3">
								<FieldBlock
									label="BIND"
									value={data.bind}
									copyText={data.bind}
								/>
								<FieldBlock
									label="XRAY API ADDR"
									value={data.xray_api_addr}
									copyText={data.xray_api_addr}
								/>
								<FieldBlock
									label="API BASE URL"
									value={data.api_base_url}
									copyText={data.api_base_url}
								/>
							</div>
						</div>
					</div>

					<div className="xp-card lg:col-span-2">
						<div className="xp-card-body space-y-4">
							<div className="flex flex-wrap items-start justify-between gap-4">
								<div className="max-w-2xl">
									<h2 className="text-base font-semibold">
										Mihomo external resource mirror
									</h2>
									<p className="text-sm text-muted-foreground">
										Cluster-wide control for private, loopback, and link-local
										upstream targets.
									</p>
								</div>
								<label
									className="flex items-center gap-3 text-sm font-medium"
									htmlFor="mihomo-private-targets"
								>
									<Checkbox
										id="mihomo-private-targets"
										checked={data.mihomo_resource_allow_private_targets}
										disabled={
											runtime.isReadOnly ||
											privateTargetPolicyMutation.isPending
										}
										aria-label="Allow private Mihomo mirror targets"
										onCheckedChange={(checked) => {
											if (checked !== "indeterminate") {
												privateTargetPolicyMutation.mutate(checked);
											}
										}}
									/>
									<span>
										{data.mihomo_resource_allow_private_targets
											? "Allowed"
											: "Blocked"}
									</span>
								</label>
							</div>
							<div
								className={
									data.mihomo_resource_allow_private_targets
										? "rounded-xl border border-warning/40 bg-warning/10 px-4 py-3 text-sm text-warning-foreground"
										: "rounded-xl border border-success/30 bg-success/10 px-4 py-3 text-sm text-success-foreground"
								}
							>
								{data.mihomo_resource_allow_private_targets
									? "Enabled: a configured mirror URL may reach private network services. Only enable this for trusted profiles."
									: "Protected: private network targets are rejected before XP connects to them."}
							</div>
						</div>
					</div>

					<div className="xp-card">
						<div className="xp-card-body space-y-3">
							<div>
								<h2 className="text-base font-semibold">Node</h2>
								<p className="text-sm text-muted-foreground">
									用于订阅与客户端连接的对外 host。
								</p>
							</div>
							<div className="space-y-3">
								<FieldBlock
									label="NODE NAME"
									value={data.node_name}
									copyText={data.node_name}
								/>
								<FieldBlock
									label="ACCESS HOST"
									value={data.access_host}
									copyText={data.access_host}
								/>
								<FieldBlock
									label="DATA DIR"
									value={data.data_dir}
									copyText={data.data_dir}
								/>
							</div>
						</div>
					</div>

					<div className="xp-card">
						<div className="xp-card-body space-y-3">
							<div>
								<h2 className="text-base font-semibold">Quota</h2>
								<p className="text-sm text-muted-foreground">
									流量统计与自动解封策略。
								</p>
							</div>
							<div className="space-y-3">
								<FieldBlock
									label="POLL INTERVAL"
									value={`${data.quota_poll_interval_secs} sec`}
									copyText={`${data.quota_poll_interval_secs}`}
								/>
								<FieldBlock
									label="AUTO UNBAN"
									value={data.quota_auto_unban ? "true" : "false"}
									copyText={data.quota_auto_unban ? "true" : "false"}
								/>
							</div>
						</div>
					</div>

					<div className="xp-card">
						<div className="xp-card-body space-y-3">
							<div>
								<h2 className="text-base font-semibold">IP Geo</h2>
								<p className="text-sm text-muted-foreground">
									入站 IP Geo enrichment 与 upstream 设置。
								</p>
							</div>
							<div className="space-y-3">
								<FieldBlock
									label="IP GEO ENABLED"
									value={data.ip_geo_enabled ? "true" : "false"}
									copyText={data.ip_geo_enabled ? "true" : "false"}
								/>
								<FieldBlock
									label="IP GEO ORIGIN"
									value={data.ip_geo_origin}
									copyText={data.ip_geo_origin}
								/>
							</div>
						</div>
					</div>

					<div className="xp-card">
						<div className="xp-card-body space-y-3">
							<div>
								<h2 className="text-base font-semibold">Security</h2>
								<p className="text-sm text-muted-foreground">
									仅展示可公开信息，敏感字段全量脱敏。
								</p>
							</div>
							<div className="space-y-3">
								<FieldBlock
									label="ADMIN TOKEN"
									value={data.admin_token_masked}
									copyText={data.admin_token_masked}
								/>
								<FieldBlock
									label="ADMIN TOKEN PRESENT"
									value={data.admin_token_present ? "true" : "false"}
									copyText={data.admin_token_present ? "true" : "false"}
								/>
							</div>
						</div>
					</div>
				</div>

				<div className="flex items-center justify-between gap-3 text-xs text-muted-foreground">
					<div>Settings / Service config</div>
					<div>
						Runtime file config is read-only · Mihomo uses provider-only
						delivery
					</div>
				</div>
			</div>
		);
	})();
	const latestSyncedAt = latestQueryDataUpdatedAt([
		configQuery,
		clusterInfo,
		health,
	]);
	const showCachedBanner =
		latestSyncedAt !== null &&
		(hasQueryData(configQuery) ||
			hasQueryData(clusterInfo) ||
			hasQueryData(health)) &&
		(!runtime.isOnline ||
			configQuery.isError ||
			clusterInfo.isError ||
			health.isError);

	return (
		<div className="space-y-5">
			<PageHeader
				title="服务配置"
				description="只读展示当前进程配置与订阅入口，便于部署核对与排障。"
				actions={headerActions}
			/>
			{showCachedBanner ? (
				<ReadStateBanner
					tone={!runtime.isOnline ? "warning" : "info"}
					variant="inline"
					dismissible
					errors={[configQuery.error, clusterInfo.error, health.error]}
					title={!runtime.isOnline ? "离线配置快照" : "当前展示的是缓存配置"}
					description={`最近一次成功同步：${formatSyncTimestamp(latestSyncedAt)}。`}
				/>
			) : null}
			{content}
		</div>
	);
}
