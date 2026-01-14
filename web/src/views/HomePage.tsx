import { useQuery } from "@tanstack/react-query";
import { useState } from "react";

import { fetchAdminAlerts } from "../api/adminAlerts";
import { verifyAdminToken } from "../api/adminAuth";
import { fetchAdminNodes } from "../api/adminNodes";
import { isBackendApiError } from "../api/backendError";
import { fetchClusterInfo } from "../api/clusterInfo";
import { fetchHealth } from "../api/health";
import { Button } from "../components/Button";
import { PageHeader } from "../components/PageHeader";
import { useUiPrefs } from "../components/UiPrefs";
import {
	ADMIN_TOKEN_STORAGE_KEY,
	clearAdminToken,
	readAdminToken,
	writeAdminToken,
} from "../components/auth";
import { parseAdminTokenInput } from "../utils/adminToken";

function formatError(err: unknown): string {
	if (isBackendApiError(err)) {
		const code = err.code ? ` ${err.code}` : "";
		return `${err.status}${code}: ${err.message}`;
	}
	if (err instanceof Error) return err.message;
	return String(err);
}

export function HomePage() {
	const prefs = useUiPrefs();
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
		queryKey: ["adminNodes", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminNodes(adminToken, signal),
	});

	const adminAlerts = useQuery({
		queryKey: ["adminAlerts", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminAlerts(adminToken, signal),
	});

	const inputClass =
		prefs.density === "compact"
			? "input input-bordered input-sm font-mono"
			: "input input-bordered font-mono";

	return (
		<div className="space-y-6">
			<PageHeader title="Dashboard" description="Control plane bootstrap UI." />

			<div className="card bg-base-100 shadow">
				<div className="card-body">
					<h2 className="card-title">Backend health</h2>
					{health.isLoading ? (
						<p>Loading...</p>
					) : health.isError ? (
						<p className="text-error">Failed to reach backend.</p>
					) : (
						<p>
							Status:{" "}
							<span className="font-mono">
								{health.data?.status ?? "unknown"}
							</span>
						</p>
					)}
					<div className="card-actions justify-end">
						<Button
							variant="secondary"
							loading={health.isFetching}
							onClick={() => health.refetch()}
						>
							Refresh
						</Button>
					</div>
				</div>
			</div>

			<div className="card bg-base-100 shadow">
				<div className="card-body">
					<h2 className="card-title">Admin token</h2>
					<p className="text-sm opacity-70">
						Stored in localStorage key{" "}
						<span className="font-mono">{ADMIN_TOKEN_STORAGE_KEY}</span>.
					</p>
					<label className="form-control">
						<div className="label">
							<span className="label-text">Token</span>
						</div>
						<input
							type="password"
							className={inputClass}
							placeholder="e.g. testtoken"
							value={adminTokenDraft}
							onChange={(e) => {
								setAdminTokenDraft(e.target.value);
								setAdminTokenError(null);
							}}
						/>
					</label>
					{adminToken.length === 0 ? (
						<p className="text-warning">
							Please set admin token to query nodes.
						</p>
					) : (
						<p className="text-sm opacity-70">
							Token is set (length {adminToken.length}).
						</p>
					)}
					{adminTokenError ? (
						<p className="text-sm text-error">{adminTokenError}</p>
					) : null}
					<div className="card-actions justify-end">
						<Button
							variant="secondary"
							loading={isSavingAdminToken}
							disabled={isSavingAdminToken}
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
					</div>
				</div>
			</div>

			<div className="card bg-base-100 shadow">
				<div className="card-body">
					<h2 className="card-title">Cluster info</h2>
					{clusterInfo.isLoading ? (
						<p>Loading...</p>
					) : clusterInfo.isError ? (
						<div className="space-y-1">
							<p className="text-error">Failed to load cluster info.</p>
							{isBackendApiError(clusterInfo.error) ? (
								<p className="font-mono text-sm opacity-70">
									{clusterInfo.error.status} {clusterInfo.error.code}:{" "}
									{clusterInfo.error.message}
								</p>
							) : null}
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
					<div className="card-actions justify-end">
						<Button
							variant="secondary"
							loading={clusterInfo.isFetching}
							onClick={() => clusterInfo.refetch()}
						>
							Refresh
						</Button>
					</div>
				</div>
			</div>

			<div className="card bg-base-100 shadow">
				<div className="card-body">
					<h2 className="card-title">Alerts</h2>
					{adminToken.length === 0 ? (
						<p className="text-warning">Please set admin token.</p>
					) : adminAlerts.isLoading ? (
						<p>Loading...</p>
					) : adminAlerts.isError ? (
						<div className="space-y-1">
							<p className="text-error">Failed to load alerts.</p>
							{isBackendApiError(adminAlerts.error) ? (
								<p className="font-mono text-sm opacity-70">
									{adminAlerts.error.status} {adminAlerts.error.code}:{" "}
									{adminAlerts.error.message}
								</p>
							) : (
								<p className="font-mono text-sm opacity-70">
									{String(adminAlerts.error)}
								</p>
							)}
						</div>
					) : !adminAlerts.data ? (
						<p className="text-sm opacity-70">No data.</p>
					) : (
						<div className="space-y-3">
							<p>
								Alerts count:{" "}
								<span className="font-mono">
									{adminAlerts.data.items.length}
								</span>
							</p>
							{adminAlerts.data.partial ? (
								<div className="space-y-1">
									<p className="text-warning font-semibold">
										Warning: results are partial due to unreachable nodes.
									</p>
									<p className="font-mono text-sm">
										unreachable_nodes:{" "}
										{adminAlerts.data.unreachable_nodes.length > 0
											? adminAlerts.data.unreachable_nodes.join(", ")
											: "none"}
									</p>
								</div>
							) : null}
							{adminAlerts.data.items.length > 0 ? (
								<div className="space-y-2">
									<p className="text-error font-semibold">
										Warning: {adminAlerts.data.items.length} alert(s) detected.
									</p>
									<div className="overflow-x-auto">
										<table className="table table-zebra table-sm">
											<thead>
												<tr>
													<th>type</th>
													<th>grant_id</th>
													<th>message</th>
													<th>action_hint</th>
												</tr>
											</thead>
											<tbody>
												{adminAlerts.data.items.map((item) => (
													<tr
														key={`${item.type}-${item.grant_id}-${item.endpoint_id}-${item.owner_node_id}`}
													>
														<td>{item.type}</td>
														<td className="font-mono">{item.grant_id}</td>
														<td>{item.message}</td>
														<td>{item.action_hint}</td>
													</tr>
												))}
											</tbody>
										</table>
									</div>
								</div>
							) : (
								<p className="text-sm opacity-70">No alerts.</p>
							)}
						</div>
					)}
					<div className="card-actions justify-end">
						<Button
							variant="secondary"
							disabled={adminToken.length === 0}
							loading={adminAlerts.isFetching}
							onClick={() => adminAlerts.refetch()}
						>
							Refresh
						</Button>
					</div>
				</div>
			</div>

			<div className="card bg-base-100 shadow">
				<div className="card-body">
					<h2 className="card-title">Nodes</h2>
					{adminToken.length === 0 ? (
						<p className="text-warning">Please set admin token.</p>
					) : adminNodes.isLoading ? (
						<p>Loading...</p>
					) : adminNodes.isError ? (
						<div className="space-y-1">
							<p className="text-error">Failed to load nodes.</p>
							{isBackendApiError(adminNodes.error) ? (
								<p className="font-mono text-sm opacity-70">
									{adminNodes.error.status} {adminNodes.error.code}:{" "}
									{adminNodes.error.message}
								</p>
							) : (
								<p className="font-mono text-sm opacity-70">
									{String(adminNodes.error)}
								</p>
							)}
						</div>
					) : !adminNodes.data ? (
						<p className="text-sm opacity-70">No data.</p>
					) : (
						<div className="overflow-x-auto">
							<table className="table table-zebra">
								<thead>
									<tr>
										<th>node_name</th>
										<th>node_id</th>
										<th>api_base_url</th>
										<th>public_domain</th>
									</tr>
								</thead>
								<tbody>
									{adminNodes.data.items.map((n) => (
										<tr key={n.node_id}>
											<td className="font-mono">{n.node_name}</td>
											<td className="font-mono">{n.node_id}</td>
											<td className="font-mono">{n.api_base_url}</td>
											<td className="font-mono">{n.public_domain}</td>
										</tr>
									))}
								</tbody>
							</table>
						</div>
					)}
					<div className="card-actions justify-end">
						<Button
							variant="secondary"
							disabled={adminToken.length === 0}
							loading={adminNodes.isFetching}
							onClick={() => adminNodes.refetch()}
						>
							Refresh
						</Button>
					</div>
				</div>
			</div>
		</div>
	);
}
