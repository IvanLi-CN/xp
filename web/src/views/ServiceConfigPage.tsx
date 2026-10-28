import { useQuery } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";

import { fetchAdminConfig } from "../api/adminConfig";
import {
	fetchAdminMeshStatus,
	parseMeshPreflightFailures,
	updateAdminMeshConfig,
} from "../api/adminMesh";
import { isBackendApiError } from "../api/backendError";
import { fetchClusterInfo } from "../api/clusterInfo";
import { fetchHealth } from "../api/health";
import {
	useApiCapability,
	useApiRouteCapability,
} from "../api/useApiCompatibility";
import { usePrimaryBackend } from "../backend/PrimaryBackendProvider";
import {
	getPrimaryBackendSnapshot,
	withPrimaryBackendMutation,
} from "../backend/primaryBackend";
import { Button } from "../components/Button";
import { CopyButton } from "../components/CopyButton";
import {
	MeshClusterSetting,
	type MeshMutationIssue,
	type MeshMutationPhase,
} from "../components/MeshClusterSetting";
import { PageHeader } from "../components/PageHeader";
import { CapabilityUnavailableState, PageState } from "../components/PageState";
import { ReadStateBanner } from "../components/ReadStateBanner";
import { useToast } from "../components/Toast";
import { readAdminToken } from "../components/auth";
import { useAppRuntime } from "../offline/appRuntime";
import {
	formatSyncTimestamp,
	hasQueryData,
	latestQueryDataUpdatedAt,
	queryIsOfflineBlocked,
} from "../offline/queryReadState";
import { useQueryWithOfflineFallback } from "../offline/useQueryWithOfflineFallback";
import {
	clearMeshMutationLock,
	readMeshMutationLock,
	readMeshMutationLocks,
	withMeshMutationLock,
} from "../utils/meshMutationLock";

const MESH_STATUS_FRESHNESS_MS = 30_000;

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

function isAmbiguousMeshMutationError(error: unknown): boolean {
	return !isBackendApiError(error) || error.status >= 500;
}

function knownMeshMutationErrorMessage(error: unknown): string {
	if (!isBackendApiError(error))
		return "The cluster setting request was rejected.";
	if (error.status === 409 && error.code === "mesh_preflight_failed") {
		return "Directed cluster path validation failed. No setting change was confirmed.";
	}
	return error.code
		? `The cluster setting request was rejected (${error.code}).`
		: `The cluster setting request was rejected (${error.status}).`;
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
	const primaryBackend = usePrimaryBackend();
	const configCapability = useApiCapability("admin.config");
	const meshCapability = useApiCapability("admin.mesh");
	const meshConfigCapability = useApiRouteCapability(
		"admin.mesh",
		"PUT /api/admin/mesh/config",
	);
	const meshDialogGeneration = useRef(0);
	const meshMutationPhaseRef = useRef<MeshMutationPhase>("idle");
	const [meshMutationPhase, setMeshMutationPhaseState] =
		useState<MeshMutationPhase>("idle");
	const [meshPendingTarget, setMeshPendingTarget] = useState<boolean | null>(
		null,
	);
	const [meshConfirmationValue, setMeshConfirmationValue] = useState<
		boolean | undefined
	>();
	const [meshMutationIssue, setMeshMutationIssue] =
		useState<MeshMutationIssue | null>(null);
	const [meshPreflightFailures, setMeshPreflightFailures] = useState<
		ReturnType<typeof parseMeshPreflightFailures>
	>([]);
	const [, setMeshLockRevision] = useState(0);
	const [meshFreshnessTick, setMeshFreshnessTick] = useState(0);
	const setMeshMutationPhase = (phase: MeshMutationPhase) => {
		meshMutationPhaseRef.current = phase;
		setMeshMutationPhaseState(phase);
	};

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
		enabled: adminToken.length > 0 && configCapability.available,
		queryFn: ({ signal }) => fetchAdminConfig(adminToken, signal),
	});

	const meshQuery = useQuery({
		queryKey: [
			"adminMeshStatus",
			adminToken,
			primaryBackend.primaryOrigin,
			primaryBackend.generation,
		],
		enabled: adminToken.length > 0 && meshCapability.available,
		queryFn: ({ signal }) => fetchAdminMeshStatus(adminToken, signal),
		refetchOnMount: runtime.isOnline,
		refetchOnWindowFocus: false,
		retry: false,
	});
	const meshState = useQueryWithOfflineFallback(
		["adminMeshStatus", adminToken, primaryBackend.primaryOrigin],
		meshQuery,
	);
	const meshData = meshCapability.available ? meshState.data : undefined;
	const meshClusterId = meshData?.local.cluster_id ?? null;
	const meshLocks = readMeshMutationLocks();
	const meshLock = readMeshMutationLock(meshClusterId);
	const meshStatusUnavailableLock =
		!meshClusterId && meshLocks.length === 1 ? (meshLocks[0] ?? null) : null;
	const meshStatusFresh =
		meshQuery.isSuccess &&
		!meshQuery.isError &&
		meshQuery.data !== undefined &&
		meshQuery.dataUpdatedAt > 0 &&
		Date.now() - meshQuery.dataUpdatedAt <= MESH_STATUS_FRESHNESS_MS &&
		meshFreshnessTick >= 0;

	useEffect(() => {
		if (!meshQuery.dataUpdatedAt) return;
		const interval = window.setInterval(
			() => setMeshFreshnessTick((tick) => tick + 1),
			1_000,
		);
		return () => window.clearInterval(interval);
	}, [meshQuery.dataUpdatedAt]);

	const closeMeshDialog = (force = false) => {
		if (!force && meshMutationPhaseRef.current === "submitting") return;
		meshDialogGeneration.current += 1;
		setMeshPendingTarget(null);
		setMeshConfirmationValue(undefined);
		setMeshMutationPhase("idle");
	};

	const refreshMeshStatus = async () => {
		const result = await meshQuery.refetch();
		if (result.data && !result.isError) {
			setMeshMutationIssue((issue) =>
				issue?.kind === "success_unconfirmed" ? null : issue,
			);
		}
		return result;
	};

	const openMeshChangeConfirmation = (enabled: boolean) => {
		if (
			!meshStatusFresh ||
			!meshData ||
			meshLock ||
			meshStatusUnavailableLock ||
			!meshConfigCapability.available
		)
			return;
		const generation = ++meshDialogGeneration.current;
		setMeshMutationIssue(null);
		setMeshPreflightFailures([]);
		setMeshPendingTarget(enabled);
		setMeshConfirmationValue(undefined);
		setMeshMutationPhase("refreshing");
		const expectedClusterId = meshData.local.cluster_id;
		const expectedOrigin = primaryBackend.primaryOrigin;
		const expectedGeneration = primaryBackend.generation;
		void meshQuery.refetch().then((result) => {
			if (generation !== meshDialogGeneration.current) return;
			const currentBackend = getPrimaryBackendSnapshot();
			if (
				result.isError ||
				!result.data ||
				result.dataUpdatedAt <= 0 ||
				Date.now() - result.dataUpdatedAt > MESH_STATUS_FRESHNESS_MS ||
				currentBackend.primaryOrigin !== expectedOrigin ||
				currentBackend.generation !== expectedGeneration ||
				result.data.local.cluster_id !== expectedClusterId
			) {
				setMeshMutationIssue({
					kind: "rejected",
					message:
						"The current backend or cluster state changed while refreshing. " +
						"Review the latest observation before trying again.",
				});
				closeMeshDialog();
				return;
			}
			setMeshConfirmationValue(result.data.cluster_mesh_enabled);
			setMeshMutationPhase("confirming");
		});
	};

	const confirmMeshChange = () => {
		if (meshPendingTarget === null || !meshData) return;
		const target = meshPendingTarget;
		const generation = meshDialogGeneration.current;
		const expectedClusterId = meshData.local.cluster_id;
		const expectedOrigin = primaryBackend.primaryOrigin;
		const expectedGeneration = primaryBackend.generation;
		setMeshMutationPhase("refreshing");
		void meshQuery.refetch().then(async (result) => {
			if (generation !== meshDialogGeneration.current) return;
			const currentBackend = getPrimaryBackendSnapshot();
			if (
				result.isError ||
				!result.data ||
				result.dataUpdatedAt <= 0 ||
				Date.now() - result.dataUpdatedAt > MESH_STATUS_FRESHNESS_MS ||
				currentBackend.primaryOrigin !== expectedOrigin ||
				currentBackend.generation !== expectedGeneration ||
				result.data.local.cluster_id !== expectedClusterId
			) {
				setMeshMutationIssue({
					kind: "rejected",
					message:
						"The current backend or cluster state changed. No mutation was sent; " +
						"review the latest observation.",
				});
				closeMeshDialog();
				return;
			}
			if (result.data.cluster_mesh_enabled === target) {
				setMeshConfirmationValue(target);
				setMeshMutationPhase("confirming");
				return;
			}

			const lock = {
				clusterId: expectedClusterId,
				targetEnabled: target,
				createdAt: Date.now(),
			};
			try {
				const lockResult = await withMeshMutationLock(
					lock,
					async ({ clear }) => {
						const currentBackend = getPrimaryBackendSnapshot();
						if (
							currentBackend.primaryOrigin !== expectedOrigin ||
							currentBackend.generation !== expectedGeneration
						) {
							const cleared = clear();
							setMeshLockRevision((revision) => revision + 1);
							setMeshMutationIssue({
								kind: cleared ? "rejected" : "persistence",
								message: cleared
									? "The selected backend changed before the mutation started. " +
										"No cluster mutation was sent."
									: "The selected backend changed before the mutation started, " +
										"but the local safety lock could not be cleared.",
							});
							return;
						}
						setMeshLockRevision((revision) => revision + 1);
						setMeshMutationPhase("submitting");
						try {
							await withPrimaryBackendMutation(() =>
								updateAdminMeshConfig(adminToken, target),
							);
							const cleared = clear();
							setMeshLockRevision((revision) => revision + 1);
							const refreshed = await meshQuery.refetch();
							if (!refreshed.data || refreshed.isError) {
								setMeshMutationIssue({
									kind: "success_unconfirmed",
									message:
										"The server accepted the setting, but the latest cluster state could not be confirmed.",
								});
							} else if (refreshed.data.cluster_mesh_enabled !== target) {
								const observedValue = refreshed.data.cluster_mesh_enabled
									? "Enabled"
									: "Disabled";
								setMeshMutationIssue({
									kind: "success_unconfirmed",
									message: [
										"The server accepted the request, but this observer currently reports",
										`${observedValue}.`,
										"Another administrator or replication delay may be involved.",
									].join(" "),
								});
							} else if (!cleared) {
								setMeshMutationIssue({
									kind: "persistence",
									message:
										"The setting was accepted, but the local safety lock could not be " +
										"cleared. Verify the cluster before unlocking it.",
								});
							} else {
								setMeshMutationIssue(null);
							}
						} catch (error) {
							if (isAmbiguousMeshMutationError(error)) {
								setMeshMutationIssue({
									kind: "unknown",
									message:
										"The mutation result is unknown. Refreshing or switching backend " +
										"cannot prove whether it was applied.",
								});
							} else {
								clear();
								setMeshLockRevision((revision) => revision + 1);
								const failures = parseMeshPreflightFailures(error);
								setMeshPreflightFailures(failures);
								setMeshMutationIssue({
									kind: failures.length > 0 ? "preflight" : "rejected",
									message: knownMeshMutationErrorMessage(error),
								});
								await meshQuery.refetch();
							}
						}
					},
				);
				if (!lockResult.acquired) {
					setMeshMutationIssue({
						kind: "persistence",
						message:
							lockResult.reason === "held"
								? "Another browser tab is already changing this cluster. No mutation was sent."
								: "The local safety lock could not be persisted. No cluster mutation was sent.",
					});
				}
			} catch {
				setMeshMutationIssue({
					kind: "unknown",
					message:
						"The mutation result is unknown. Refreshing or switching backend " +
						"cannot prove whether it was applied.",
				});
			}
			closeMeshDialog(true);
		});
	};

	const unlockMeshMutation = () => {
		const clusterId =
			meshLock?.clusterId ?? meshStatusUnavailableLock?.clusterId;
		if (!clusterId) return;
		void clearMeshMutationLock(clusterId).then((cleared) => {
			if (cleared) {
				setMeshLockRevision((revision) => revision + 1);
				setMeshMutationIssue(null);
				return;
			}
			setMeshMutationIssue({
				kind: "persistence",
				message:
					"The local safety lock could not be cleared. Keep it locked until storage is available.",
			});
		});
	};

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
				disabled={
					adminToken.length === 0 ||
					configCapability.unavailable ||
					runtime.isReadOnly
				}
				onClick={() => configQuery.refetch()}
			>
				Refresh
			</Button>
		</>
	);

	const content = (() => {
		if (configCapability.unavailable) {
			return (
				<CapabilityUnavailableState
					title="Service configuration unavailable"
					reason={configCapability.reason}
				/>
			);
		}
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

				<section
					aria-labelledby="cluster-settings-heading"
					className="space-y-3"
				>
					<div>
						<h2 id="cluster-settings-heading" className="text-lg font-semibold">
							Cluster settings
						</h2>
						<p className="text-sm text-muted-foreground">
							No cluster-wide Mihomo authorization is configured here.
						</p>
					</div>
					<div className="xp-card">
						<div className="xp-card-body space-y-2">
							<h3 className="text-base font-semibold">
								Mihomo resource access
							</h3>
							<p className="text-sm text-muted-foreground">
								Private mirror targets are configured per node from the node
								details page. Deployment defaults come from
								<span className="mx-1 break-all font-mono">
									XP_MIHOMO_ALLOWED_PRIVATE_CIDRS
								</span>
								and are never stored in Raft.
							</p>
						</div>
					</div>
					<MeshClusterSetting
						status={meshData}
						statusUpdatedAt={meshState.dataUpdatedAt || null}
						statusFresh={meshStatusFresh}
						statusRefreshing={meshQuery.isFetching}
						statusRefreshFailed={meshQuery.isError}
						observerBackend={primaryBackend.primaryOrigin}
						adminTokenPresent={adminToken.length > 0}
						capabilityAvailable={meshConfigCapability.available}
						capabilityReason={meshConfigCapability.reason}
						isOnline={runtime.isOnline}
						isReadOnly={runtime.isReadOnly}
						mutationPhase={meshMutationPhase}
						pendingTarget={meshPendingTarget}
						currentConfirmationValue={meshConfirmationValue}
						mutationIssue={meshMutationIssue}
						preflightFailures={meshPreflightFailures}
						unknownLock={meshLock}
						statusUnavailableLock={meshStatusUnavailableLock}
						onRequestChange={openMeshChangeConfirmation}
						onConfirm={confirmMeshChange}
						onCancel={closeMeshDialog}
						onRefresh={() => void refreshMeshStatus()}
						onUnlock={unlockMeshMutation}
					/>
				</section>

				<section aria-labelledby="node-settings-heading" className="space-y-3">
					<div>
						<h2 id="node-settings-heading" className="text-lg font-semibold">
							Node settings
						</h2>
						<p className="text-sm text-muted-foreground">
							Effective runtime values for this node. These fields are
							read-only.
						</p>
					</div>
					<div className="grid gap-4 lg:grid-cols-2">
						<div className="xp-card">
							<div className="xp-card-body space-y-3">
								<div>
									<h3 className="text-base font-semibold">Network</h3>
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

						<div className="xp-card">
							<div className="xp-card-body space-y-3">
								<div>
									<h3 className="text-base font-semibold">Node</h3>
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
									<h3 className="text-base font-semibold">Quota</h3>
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
									<h3 className="text-base font-semibold">IP Geo</h3>
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
									<h3 className="text-base font-semibold">Security</h3>
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
				</section>

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
