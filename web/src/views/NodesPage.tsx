import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useMemo, useState } from "react";

import { createAdminJoinToken } from "../api/adminJoinTokens";
import { fetchAdminNodes } from "../api/adminNodes";
import { isBackendApiError } from "../api/backendError";
import { Button } from "../components/Button";
import { CopyButton } from "../components/CopyButton";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
import { ResourceTable } from "../components/ResourceTable";
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

export function NodesPage() {
	const [adminToken] = useState(() => readAdminToken());
	const { pushToast } = useToast();
	const prefs = useUiPrefs();
	const [ttlSeconds, setTtlSeconds] = useState(3600);
	const [joinToken, setJoinToken] = useState<string | null>(null);
	const [joinTokenError, setJoinTokenError] = useState<string | null>(null);
	const [isCreatingJoinToken, setIsCreatingJoinToken] = useState(false);

	const nodesQuery = useQuery({
		queryKey: ["adminNodes", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminNodes(adminToken, signal),
	});

	const joinCommand = useMemo(() => {
		return joinToken ? `xp join --token ${joinToken}` : "";
	}, [joinToken]);

	const handleCreateJoinToken = async () => {
		setJoinTokenError(null);
		if (adminToken.length === 0) {
			setJoinTokenError("Admin token is missing.");
			return;
		}
		if (ttlSeconds <= 0 || Number.isNaN(ttlSeconds)) {
			setJoinTokenError("TTL must be greater than zero.");
			return;
		}

		setIsCreatingJoinToken(true);
		try {
			const response = await createAdminJoinToken(adminToken, {
				ttl_seconds: ttlSeconds,
			});
			setJoinToken(response.join_token);
		} catch (error) {
			const message = formatErrorMessage(error);
			setJoinTokenError(message);
			pushToast({
				variant: "error",
				message: "Failed to create join token.",
			});
		} finally {
			setIsCreatingJoinToken(false);
		}
	};

	const nodesContent = (() => {
		if (adminToken.length === 0) {
			return (
				<PageState
					variant="empty"
					title="Admin token required"
					description="Please provide an admin token to load nodes."
				/>
			);
		}

		if (nodesQuery.isLoading) {
			return (
				<PageState
					variant="loading"
					title="Loading nodes"
					description="Fetching nodes from the xp API."
				/>
			);
		}

		if (nodesQuery.isError) {
			return (
				<PageState
					variant="error"
					title="Failed to load nodes"
					description={formatErrorMessage(nodesQuery.error)}
					action={
						<Button
							variant="secondary"
							loading={nodesQuery.isFetching}
							onClick={() => nodesQuery.refetch()}
						>
							Retry
						</Button>
					}
				/>
			);
		}

		const nodes = nodesQuery.data?.items ?? [];
		if (nodes.length === 0) {
			return (
				<PageState
					variant="empty"
					title="No nodes yet"
					description="No nodes have been registered in this cluster."
					action={
						<Button
							variant="secondary"
							loading={nodesQuery.isFetching}
							onClick={() => nodesQuery.refetch()}
						>
							Refresh
						</Button>
					}
				/>
			);
		}

		return (
			<div className="space-y-3">
				<div className="flex items-center justify-between gap-3">
					<p className="text-sm opacity-70">
						{nodes.length} node{nodes.length === 1 ? "" : "s"} total
					</p>
					<Button
						variant="secondary"
						loading={nodesQuery.isFetching}
						onClick={() => nodesQuery.refetch()}
					>
						Refresh
					</Button>
				</div>
				<ResourceTable
					headers={[
						{ key: "node_id", label: "Node ID" },
						{ key: "node_name", label: "Name" },
						{ key: "access_host", label: "Access host" },
						{ key: "api_base_url", label: "API base URL" },
					]}
				>
					{nodes.map((node) => (
						<tr key={node.node_id}>
							<td className="font-mono text-sm">
								<Link
									to="/nodes/$nodeId"
									params={{ nodeId: node.node_id }}
									className="link link-primary"
								>
									{node.node_id}
								</Link>
							</td>
							<td>
								<Link
									to="/nodes/$nodeId"
									params={{ nodeId: node.node_id }}
									className="link link-hover"
								>
									{node.node_name || "(unnamed)"}
								</Link>
							</td>
							<td className="font-mono text-sm">{node.access_host || "-"}</td>
							<td className="font-mono text-sm">{node.api_base_url || "-"}</td>
						</tr>
					))}
				</ResourceTable>
			</div>
		);
	})();

	return (
		<div className="space-y-6">
			<PageHeader
				title="Nodes"
				description="Inspect cluster nodes and issue join tokens for new members."
			/>

			<div className="card bg-base-100 shadow">
				<div className="card-body space-y-4">
					<div>
						<h2 className="card-title">Join token</h2>
						<p className="text-sm opacity-70">
							Generate a token and share it with the node you want to join.
						</p>
					</div>
					<div className="grid gap-4 md:grid-cols-[minmax(0,1fr)_auto] md:items-end">
						<label className="form-control">
							<div className="label">
								<span className="label-text">TTL (seconds)</span>
							</div>
							<input
								type="number"
								min={60}
								step={60}
								className={
									prefs.density === "compact"
										? "input input-bordered input-sm font-mono"
										: "input input-bordered font-mono"
								}
								value={ttlSeconds}
								onChange={(event) => {
									const next = Number(event.target.value);
									setTtlSeconds(Number.isFinite(next) ? next : 0);
								}}
							/>
						</label>
						<div className="flex md:justify-end">
							<Button
								variant="secondary"
								loading={isCreatingJoinToken}
								disabled={ttlSeconds <= 0 || adminToken.length === 0}
								onClick={handleCreateJoinToken}
							>
								Create token
							</Button>
						</div>
					</div>
					{joinTokenError ? (
						<p className="text-sm text-error font-mono">{joinTokenError}</p>
					) : null}
					{joinToken ? (
						<div className="space-y-4 rounded-box bg-base-200 p-4">
							<div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
								<div className="space-y-1">
									<p className="text-xs uppercase tracking-wide opacity-60">
										Join token
									</p>
									<p className="font-mono text-sm break-all">{joinToken}</p>
								</div>
								<CopyButton text={joinToken} label="Copy token" />
							</div>
							<div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
								<div className="space-y-1">
									<p className="text-xs uppercase tracking-wide opacity-60">
										xp join command
									</p>
									<p className="font-mono text-sm break-all">{joinCommand}</p>
								</div>
								<CopyButton text={joinCommand} label="Copy command" />
							</div>
						</div>
					) : null}
				</div>
			</div>

			<div className="card bg-base-100 shadow">
				<div className="card-body space-y-4">
					<h2 className="card-title">Node inventory</h2>
					{nodesContent}
				</div>
			</div>
		</div>
	);
}
