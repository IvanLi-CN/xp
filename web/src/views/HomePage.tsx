import { useQuery } from "@tanstack/react-query";
import { useState } from "react";

import { fetchAdminNodes } from "../api/adminNodes";
import { isBackendApiError } from "../api/backendError";
import { fetchClusterInfo } from "../api/clusterInfo";
import { fetchHealth } from "../api/health";
import { Button } from "../components/Button";

const ADMIN_TOKEN_STORAGE_KEY = "xp_admin_token";

function readStoredAdminToken(): string {
	try {
		return localStorage.getItem(ADMIN_TOKEN_STORAGE_KEY) ?? "";
	} catch {
		return "";
	}
}

export function HomePage() {
	const [adminToken, setAdminToken] = useState(() => readStoredAdminToken());
	const [adminTokenDraft, setAdminTokenDraft] = useState(() =>
		readStoredAdminToken(),
	);

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

	return (
		<div className="space-y-6">
			<div>
				<h1 className="text-2xl font-bold">xp</h1>
				<p className="text-sm opacity-70">Control plane bootstrap UI.</p>
			</div>

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
							className="input input-bordered font-mono"
							placeholder="e.g. testtoken"
							value={adminTokenDraft}
							onChange={(e) => setAdminTokenDraft(e.target.value)}
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
					<div className="card-actions justify-end">
						<Button
							variant="secondary"
							onClick={() => {
								const next = adminTokenDraft.trim();
								localStorage.setItem(ADMIN_TOKEN_STORAGE_KEY, next);
								setAdminToken(next);
								setAdminTokenDraft(next);
							}}
						>
							Save
						</Button>
						<Button
							variant="ghost"
							onClick={() => {
								localStorage.removeItem(ADMIN_TOKEN_STORAGE_KEY);
								setAdminToken("");
								setAdminTokenDraft("");
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
