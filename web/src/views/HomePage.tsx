import { useQuery } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { useState } from "react";

import { fetchAdminAlerts } from "../api/adminAlerts";
import { verifyAdminToken } from "../api/adminAuth";
import { fetchAdminNodesRuntime } from "../api/adminNodeRuntime";
import { isBackendApiError } from "../api/backendError";
import { fetchClusterInfo } from "../api/clusterInfo";
import { fetchHealth } from "../api/health";
import { AuthRecoveryAction } from "../components/AuthRecoveryAction";
import { Button } from "../components/Button";
import { NodeInventoryList } from "../components/NodeInventoryList";
import { PageHeader } from "../components/PageHeader";
import { ReadStateBanner } from "../components/ReadStateBanner";
import { ResourceTable } from "../components/ResourceTable";
import { useUiPrefs } from "../components/UiPrefs";
import {
	ADMIN_TOKEN_STORAGE_KEY,
	clearAdminToken,
	readAdminToken,
	writeAdminToken,
} from "../components/auth";
import {
	alertClass,
	inputClass as inputControlClass,
} from "../components/ui-helpers";
import { Input } from "../components/ui/input";
import { useAppRuntime } from "../offline/appRuntime";
import {
	formatSyncTimestamp,
	hasQueryData,
	latestQueryDataUpdatedAt,
} from "../offline/queryReadState";
import { parseAdminTokenInput } from "../utils/adminToken";

function formatError(err: unknown): string {
	if (isBackendApiError(err)) {
		const code = err.code ? ` ${err.code}` : "";
		return `${err.status}${code}: ${err.message}`;
	}
	if (err instanceof Error) return err.message;
	return String(err);
}

function DashboardCard(props: {
	title: string;
	children: ReactNode;
	actions?: ReactNode;
}) {
	return (
		<div className="xp-card">
			<div className="xp-card-body">
				<h2 className="xp-card-title">{props.title}</h2>
				{props.children}
				{props.actions ? (
					<div className="xp-card-actions justify-end">{props.actions}</div>
				) : null}
			</div>
		</div>
	);
}

export function HomePage() {
	const prefs = useUiPrefs();
	const runtime = useAppRuntime();
	const [adminToken, setAdminToken] = useState(() => readAdminToken());
	const [adminTokenDraft, setAdminTokenDraft] = useState(() =>
		readAdminToken(),
	);
	const [adminTokenError, setAdminTokenError] = useState<string | null>(null);
	const [isSavingAdminToken, setIsSavingAdminToken] = useState(false);

	const health = useQuery({
		queryKey: ["health"],
		queryFn: ({ signal }) => fetchHealth(signal),
	});

	const clusterInfo = useQuery({
		queryKey: ["clusterInfo"],
		queryFn: ({ signal }) => fetchClusterInfo(signal),
	});

	const adminNodes = useQuery({
		queryKey: ["adminNodesRuntime", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminNodesRuntime(adminToken, signal),
	});

	const adminAlerts = useQuery({
		queryKey: ["adminAlerts", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminAlerts(adminToken, signal),
	});

	const latestSyncedAt = latestQueryDataUpdatedAt([
		health,
		clusterInfo,
		adminAlerts,
		adminNodes,
	]);
	const showCachedBanner =
		latestSyncedAt !== null &&
		(!runtime.isOnline ||
			health.isError ||
			clusterInfo.isError ||
			adminAlerts.isError ||
			adminNodes.isError);

	return (
		<div className="space-y-6">
			<PageHeader title="Dashboard" description="Cluster bootstrap UI." />
			{showCachedBanner ? (
				<ReadStateBanner
					tone={!runtime.isOnline ? "warning" : "info"}
					variant="inline"
					dismissible
					error={
						health.error ??
						clusterInfo.error ??
						adminAlerts.error ??
						adminNodes.error
					}
					title={
						!runtime.isOnline
							? "Offline read-only view"
							: "Showing cached dashboard data"
					}
					description={`Last successful sync: ${formatSyncTimestamp(latestSyncedAt)}.`}
				/>
			) : null}

			<DashboardCard
				title="Backend health"
				actions={
					<Button
						variant="secondary"
						loading={health.isFetching}
						onClick={() => health.refetch()}
					>
						Refresh
					</Button>
				}
			>
				{health.isLoading && !hasQueryData(health) ? (
					<p>Loading...</p>
				) : health.isError && !hasQueryData(health) ? (
					<p className="text-destructive">Failed to reach backend.</p>
				) : (
					<p>
						Status:{" "}
						<span className="font-mono">
							{health.data?.status ?? "unknown"}
						</span>
					</p>
				)}
			</DashboardCard>

			<DashboardCard
				title="Admin token"
				actions={
					<>
						<Button
							variant="secondary"
							loading={isSavingAdminToken}
							disabled={isSavingAdminToken || runtime.isReadOnly}
							onClick={async () => {
								const parsed = parseAdminTokenInput(adminTokenDraft);
								if ("error" in parsed) {
									setAdminTokenError(parsed.error);
									return;
								}
								setIsSavingAdminToken(true);
								setAdminTokenError(null);
								try {
									await verifyAdminToken(parsed.token);
									writeAdminToken(parsed.token);
									setAdminToken(parsed.token);
									setAdminTokenDraft(parsed.token);
								} catch (err) {
									setAdminTokenError(formatError(err));
								} finally {
									setIsSavingAdminToken(false);
								}
							}}
						>
							Save
						</Button>
						<Button
							variant="ghost"
							onClick={() => {
								clearAdminToken();
								setAdminToken("");
								setAdminTokenDraft("");
								setAdminTokenError(null);
							}}
						>
							Clear
						</Button>
					</>
				}
			>
				<p className="text-sm text-muted-foreground">
					Stored in localStorage key{" "}
					<span className="font-mono">{ADMIN_TOKEN_STORAGE_KEY}</span>.
				</p>
				<div className="xp-field-stack">
					<span className="text-sm font-medium">Token</span>
					<Input
						aria-label="Token"
						type="password"
						className={inputControlClass(prefs.density, "font-mono")}
						placeholder="e.g. testtoken"
						value={adminTokenDraft}
						onChange={(event) => {
							setAdminTokenDraft(event.target.value);
							setAdminTokenError(null);
						}}
					/>
				</div>
				{adminToken.length === 0 ? (
					<p className="text-warning">Please set admin token to query nodes.</p>
				) : (
					<p className="text-sm text-muted-foreground">
						Token is set (length {adminToken.length}).
					</p>
				)}
				{adminTokenError ? (
					<p className="font-mono text-sm text-destructive">
						{adminTokenError}
					</p>
				) : null}
			</DashboardCard>

			<DashboardCard
				title="Cluster info"
				actions={
					<Button
						variant="secondary"
						loading={clusterInfo.isFetching}
						onClick={() => clusterInfo.refetch()}
					>
						Refresh
					</Button>
				}
			>
				{clusterInfo.isLoading && !hasQueryData(clusterInfo) ? (
					<p>Loading...</p>
				) : clusterInfo.isError && !hasQueryData(clusterInfo) ? (
					<div className="space-y-1">
						<p className="text-destructive">Failed to load cluster info.</p>
						{isBackendApiError(clusterInfo.error) ? (
							<p className="font-mono text-sm text-muted-foreground">
								{clusterInfo.error.status} {clusterInfo.error.code}:{" "}
								{clusterInfo.error.message}
							</p>
						) : null}
						<AuthRecoveryAction error={clusterInfo.error} />
					</div>
				) : (
					<div className="space-y-1">
						<p>
							Role:{" "}
							<span className="font-mono">
								{clusterInfo.data?.role ?? "unknown"}
							</span>
						</p>
						<p>
							Node ID:{" "}
							<span className="font-mono">
								{clusterInfo.data?.node_id ?? "unknown"}
							</span>
						</p>
						<p>
							Leader API:{" "}
							<span className="font-mono">
								{clusterInfo.data?.leader_api_base_url ?? "unknown"}
							</span>
						</p>
						<p>
							Term:{" "}
							<span className="font-mono">
								{clusterInfo.data?.term ?? "unknown"}
							</span>
						</p>
					</div>
				)}
			</DashboardCard>

			<DashboardCard
				title="Alerts"
				actions={
					<Button
						variant="secondary"
						disabled={adminToken.length === 0}
						loading={adminAlerts.isFetching}
						onClick={() => adminAlerts.refetch()}
					>
						Refresh
					</Button>
				}
			>
				{adminToken.length === 0 ? (
					<p className="text-warning">Please set admin token.</p>
				) : adminAlerts.isLoading && !hasQueryData(adminAlerts) ? (
					<p>Loading...</p>
				) : adminAlerts.isError && !hasQueryData(adminAlerts) ? (
					<div className="space-y-1">
						<p className="text-destructive">Failed to load alerts.</p>
						<p className="font-mono text-sm text-muted-foreground">
							{formatError(adminAlerts.error)}
						</p>
						<AuthRecoveryAction error={adminAlerts.error} />
					</div>
				) : !adminAlerts.data ? (
					<p className="text-sm text-muted-foreground">No data.</p>
				) : (
					<div className="space-y-3">
						<p>
							Alerts count:{" "}
							<span className="font-mono">{adminAlerts.data.items.length}</span>
						</p>
						{adminAlerts.data.partial ? (
							<div className={alertClass("warning")}>
								<div className="space-y-1">
									<p className="font-semibold">
										Warning: results are partial due to unreachable nodes.
									</p>
									<p className="font-mono text-sm">
										unreachable_nodes:{" "}
										{adminAlerts.data.unreachable_nodes.length > 0
											? adminAlerts.data.unreachable_nodes.join(", ")
											: "none"}
									</p>
								</div>
							</div>
						) : null}
						{adminAlerts.data.items.length > 0 ? (
							<div className="space-y-3">
								<div className={alertClass("error")}>
									<p className="font-semibold">
										Warning: {adminAlerts.data.items.length} alert(s) detected.
									</p>
								</div>
								<ResourceTable
									headers={[
										{ key: "type", label: "type" },
										{ key: "membership_key", label: "membership_key" },
										{ key: "message", label: "message" },
										{ key: "action_hint", label: "action_hint" },
									]}
								>
									{adminAlerts.data.items.map((item) => (
										<tr
											key={`${item.type}-${item.membership_key}-${item.owner_node_id}`}
										>
											<td>{item.type}</td>
											<td className="font-mono text-xs">
												{item.membership_key}
											</td>
											<td>{item.message}</td>
											<td>{item.action_hint}</td>
										</tr>
									))}
								</ResourceTable>
							</div>
						) : (
							<p className="text-sm text-muted-foreground">No alerts.</p>
						)}
					</div>
				)}
			</DashboardCard>

			<DashboardCard title="Nodes">
				{adminToken.length === 0 ? (
					<p className="text-warning">Please set admin token.</p>
				) : adminNodes.isLoading && !hasQueryData(adminNodes) ? (
					<p>Loading...</p>
				) : adminNodes.isError && !hasQueryData(adminNodes) ? (
					<div className="space-y-3">
						<p className="text-destructive">Failed to load nodes.</p>
						<p className="font-mono text-sm text-muted-foreground">
							{formatError(adminNodes.error)}
						</p>
						<div className="xp-card-actions justify-end">
							<AuthRecoveryAction error={adminNodes.error} />
							<Button
								variant="secondary"
								loading={adminNodes.isFetching}
								onClick={() => adminNodes.refetch()}
							>
								Retry
							</Button>
						</div>
					</div>
				) : !adminNodes.data ? (
					<div className="space-y-3">
						<p className="text-sm text-muted-foreground">No data.</p>
						<div className="xp-card-actions justify-end">
							<Button
								variant="secondary"
								loading={adminNodes.isFetching}
								onClick={() => adminNodes.refetch()}
							>
								Refresh
							</Button>
						</div>
					</div>
				) : (
					<NodeInventoryList
						items={adminNodes.data.items}
						partial={adminNodes.data.partial}
						unreachableNodes={adminNodes.data.unreachable_nodes}
						isRefreshing={adminNodes.isFetching}
						onRefresh={() => adminNodes.refetch()}
					/>
				)}
			</DashboardCard>
		</div>
	);
}
