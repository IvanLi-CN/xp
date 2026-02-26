import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useMemo, useState } from "react";

import { createAdminJoinToken } from "../api/adminJoinTokens";
import {
	type AdminNodeRuntimeListItem,
	fetchAdminNodesRuntime,
} from "../api/adminNodeRuntime";
import { isBackendApiError } from "../api/backendError";
import { fetchClusterInfo } from "../api/clusterInfo";
import { Button } from "../components/Button";
import { CopyButton } from "../components/CopyButton";
import { Icon } from "../components/Icon";
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

function summaryBadgeClass(
	status: AdminNodeRuntimeListItem["summary"]["status"],
): string {
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

function historySlotClass(status: string): string {
	switch (status) {
		case "up":
			return "bg-success";
		case "degraded":
			return "bg-warning";
		case "down":
			return "bg-error";
		default:
			return "bg-base-300";
	}
}

function highlightShell(text: string) {
	const regex =
		/(\$\{[^}]+\}|\$[A-Za-z_][A-Za-z0-9_]*|'[^']*'|"[^"]*"|https?:\/\/[^\s"']+|--[a-z0-9-]+)/g;
	const parts = text.split(regex);
	let offset = 0;

	return parts.map((part) => {
		if (part.length === 0) return null;
		const key = `o${offset}`;
		offset += part.length;

		let className: string | null = null;
		if (part.startsWith("http://") || part.startsWith("https://")) {
			className = "text-info";
		} else if (part.startsWith("--")) {
			className = "text-warning";
		} else if (part.startsWith("$")) {
			className = "text-accent";
		} else if (part.startsWith("'") || part.startsWith('"')) {
			className = "text-success";
		}

		return className ? (
			<span key={key} className={className}>
				{part}
			</span>
		) : (
			<span key={key}>{part}</span>
		);
	});
}

export function NodesPage() {
	const [adminToken] = useState(() => readAdminToken());
	const { pushToast } = useToast();
	const prefs = useUiPrefs();
	const [ttlSeconds, setTtlSeconds] = useState(3600);
	const [joinToken, setJoinToken] = useState<string | null>(null);
	const [joinTokenError, setJoinTokenError] = useState<string | null>(null);
	const [isCreatingJoinToken, setIsCreatingJoinToken] = useState(false);

	const clusterInfoQuery = useQuery({
		queryKey: ["clusterInfo"],
		queryFn: ({ signal }) => fetchClusterInfo(signal),
	});

	const nodesQuery = useQuery({
		queryKey: ["adminNodesRuntime", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminNodesRuntime(adminToken, signal),
	});

	const joinCommand = useMemo(() => {
		return joinToken ? `xp join --token ${joinToken}` : "";
	}, [joinToken]);

	const deployCommand = useMemo(() => {
		if (!joinToken) return "";
		const xpVersion = clusterInfoQuery.data?.xp_version;
		if (!xpVersion) return "";

		const tag = xpVersion.startsWith("v") ? xpVersion : `v${xpVersion}`;

		return [
			"set -euo pipefail",
			`XP_VERSION='${xpVersion}'`,
			'XP_REPO="${XP_REPO:-IvanLi-CN/xp}"',
			"",
			'arch="$(uname -m)"',
			'case "$arch" in',
			"  x86_64|amd64) platform=x86_64 ;;",
			"  aarch64|arm64) platform=aarch64 ;;",
			'  *) echo "unsupported arch: $arch" >&2; exit 2 ;;',
			"esac",
			"",
			`tag='${tag}'`,
			'tmp_dir="$(mktemp -d)"',
			"trap 'rm -rf \"$tmp_dir\"' EXIT",
			"",
			'curl -fsSL "https://github.com/${XP_REPO}/releases/download/${tag}/xp-ops-linux-${platform}" -o "$tmp_dir/xp-ops"',
			'curl -fsSL "https://github.com/${XP_REPO}/releases/download/${tag}/xp-linux-${platform}" -o "$tmp_dir/xp"',
			'sudo install -m 0755 "$tmp_dir/xp-ops" /usr/local/bin/xp-ops',
			'sudo install -m 0755 "$tmp_dir/xp" /usr/local/bin/xp',
			"",
			'NODE_NAME="${NODE_NAME:-$(hostname -s 2>/dev/null || hostname)}"',
			'ACCESS_HOST="${ACCESS_HOST:-$(hostname -f 2>/dev/null || hostname)}"',
			'API_BASE_URL="${API_BASE_URL:-https://${ACCESS_HOST}:62416}"',
			"",
			`sudo xp-ops deploy --no-cloudflare --api-base-url \"$API_BASE_URL\" --node-name \"$NODE_NAME\" --access-host \"$ACCESS_HOST\" --join-token '${joinToken}' --enable-services --non-interactive -y`,
		].join("\n");
	}, [joinToken, clusterInfoQuery.data?.xp_version]);

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

		const unreachable = nodesQuery.data?.unreachable_nodes ?? [];
		const partial = nodesQuery.data?.partial ?? false;

		return (
			<div className="space-y-3">
				{partial ? (
					<div className="alert alert-warning">
						<span className="text-sm">
							Partial result: unreachable node(s):{" "}
							<span className="font-mono">{unreachable.join(", ") || "-"}</span>
						</span>
					</div>
				) : null}
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
						{ key: "summary", label: "Summary" },
						{ key: "components", label: "Components" },
						{ key: "recent_slots", label: "7d (30m)" },
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
							<td>
								<span className={summaryBadgeClass(node.summary.status)}>
									{node.summary.status}
								</span>
							</td>
							<td>
								<div className="flex flex-wrap gap-1">
									{node.components.map((component) => (
										<span
											key={component.component}
											className={componentBadgeClass(component.status)}
											title={`${component.component}: ${component.status}`}
										>
											{component.component}:{component.status}
										</span>
									))}
								</div>
							</td>
							<td>
								<div className="flex items-end gap-px min-w-56">
									{node.recent_slots.map((slot) => (
										<div
											key={slot.slot_start}
											className={`h-4 w-[1px] ${historySlotClass(slot.status)}`}
											title={`${slot.slot_start} • ${slot.status}`}
										/>
									))}
								</div>
							</td>
							<td className="font-mono text-sm">{node.access_host || "-"}</td>
							<td className="font-mono text-sm">
								{node.api_base_url ? (
									<a
										href={node.api_base_url}
										className="link link-primary inline-flex items-center gap-1"
									>
										<span aria-hidden="true">
											<Icon
												name="tabler:external-link"
												size={16}
												className="opacity-70"
											/>
										</span>
										{node.api_base_url}
									</a>
								) : (
									"-"
								)}
							</td>
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
							<div className="grid gap-4 lg:grid-cols-12">
								<div className="space-y-3 rounded-box bg-base-100/60 p-4 lg:col-span-6">
									<div className="flex items-center justify-between gap-2">
										<p className="text-xs uppercase tracking-wide opacity-60">
											Join token
										</p>
										<CopyButton
											text={joinToken}
											ariaLabel="Copy join token"
											iconOnly
											variant="ghost"
											size="sm"
										/>
									</div>
									<p className="font-mono text-sm break-all">{joinToken}</p>
								</div>

								<div className="space-y-3 rounded-box bg-base-100/60 p-4 lg:col-span-6">
									<div className="flex items-center justify-between gap-2">
										<p className="text-xs uppercase tracking-wide opacity-60">
											xp join command (legacy)
										</p>
										<CopyButton
											text={joinCommand}
											ariaLabel="Copy join command"
											iconOnly
											variant="ghost"
											size="sm"
										/>
									</div>
									<p className="font-mono text-sm break-all">{joinCommand}</p>
								</div>

								<div className="space-y-3 rounded-box bg-base-100/60 p-4 lg:col-span-12">
									<div className="space-y-1 min-w-0">
										<div className="flex items-center justify-between gap-2">
											<p className="text-xs uppercase tracking-wide opacity-60">
												xp-ops deploy command (recommended)
											</p>
											<CopyButton
												text={deployCommand || ""}
												ariaLabel="Copy deploy command"
												iconOnly
												variant="ghost"
												size="sm"
											/>
										</div>
										{deployCommand ? (
											<pre
												className={
													prefs.density === "compact"
														? "rounded-box border border-base-content/20 bg-base-100/40 p-3 font-mono text-sm leading-5 max-h-72 overflow-auto"
														: "rounded-box border border-base-content/20 bg-base-100/40 p-3 font-mono text-sm leading-5 max-h-72 overflow-auto"
												}
											>
												{highlightShell(deployCommand)}
											</pre>
										) : (
											<p className="text-sm opacity-70">
												Loading cluster version...
											</p>
										)}
									</div>
								</div>
							</div>
							<div className="text-sm opacity-70">
								<p>
									Notes: you can override{" "}
									<span className="font-mono">XP_REPO</span>,{" "}
									<span className="font-mono">NODE_NAME</span>,{" "}
									<span className="font-mono">ACCESS_HOST</span>, and{" "}
									<span className="font-mono">API_BASE_URL</span> before running
									the deploy command.
								</p>
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
