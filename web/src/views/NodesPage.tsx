import { useQuery } from "@tanstack/react-query";
import { useLocation, useNavigate } from "@tanstack/react-router";
import { useMemo, useState } from "react";

import { createAdminJoinToken } from "../api/adminJoinTokens";
import { fetchAdminNodesRuntime } from "../api/adminNodeRuntime";
import { fetchClusterInfo } from "../api/clusterInfo";
import { useApiCapability } from "../api/useApiCompatibility";
import { Button } from "../components/Button";
import { HistoryRepositoriesPanel } from "../components/HistoryRepositoriesPanel";
import { JoinNodePanel } from "../components/JoinNodePanel";
import {
	ModuleTabsLayout,
	ModuleTabsPanel,
} from "../components/ModuleTabsLayout";
import { NodeInventoryList } from "../components/NodeInventoryList";
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
import { formatBackendError } from "../utils/backendErrorMessage";

type NodesTab = "nodes" | "join" | "repositories";
type NodesTabPath = "/nodes" | "/nodes/join" | "/nodes/repositories";

const NODES_TAB_OPTIONS = [
	{ value: "nodes", label: "节点" },
	{ value: "join", label: "加入节点" },
	{ value: "repositories", label: "历史仓库" },
] satisfies Array<{ value: NodesTab; label: string }>;

const NODES_TAB_PATHS: Record<NodesTab, NodesTabPath> = {
	nodes: "/nodes",
	join: "/nodes/join",
	repositories: "/nodes/repositories",
};

function nodesTabFromPath(pathname: string): NodesTab {
	if (pathname === NODES_TAB_PATHS.join) return "join";
	if (pathname === NODES_TAB_PATHS.repositories) return "repositories";
	return "nodes";
}

export function NodesPage() {
	const location = useLocation();
	const navigate = useNavigate();
	const [adminToken] = useState(() => readAdminToken());
	const runtime = useAppRuntime();
	const nodesCapability = useApiCapability("admin.nodes");
	const { pushToast } = useToast();
	const [ttlSeconds, setTtlSeconds] = useState(3600);
	const [joinToken, setJoinToken] = useState<string | null>(null);
	const [joinTokenError, setJoinTokenError] = useState<string | null>(null);
	const [isCreatingJoinToken, setIsCreatingJoinToken] = useState(false);

	const clusterInfoQuery = useQuery({
		queryKey: ["clusterInfo"],
		queryFn: ({ signal }) => fetchClusterInfo(signal),
	});
	const clusterInfoState = useQueryWithOfflineFallback(
		["clusterInfo"],
		clusterInfoQuery,
	);
	const nodesQuery = useQuery({
		queryKey: ["adminNodesRuntime", adminToken],
		enabled:
			adminToken.length > 0 && (nodesCapability.available || !runtime.isOnline),
		queryFn: ({ signal }) => fetchAdminNodesRuntime(adminToken, signal),
	});
	const nodesState = useQueryWithOfflineFallback(
		["adminNodesRuntime", adminToken],
		nodesQuery,
	);
	const joinCommand = useMemo(() => {
		return joinToken ? `xp join --token ${joinToken}` : "";
	}, [joinToken]);

	const deployCommand = useMemo(() => {
		if (!joinToken) return "";
		const xpVersion = clusterInfoState.data?.xp_version;
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
	}, [joinToken, clusterInfoState.data?.xp_version]);

	const handleCreateJoinToken = async () => {
		setJoinTokenError(null);
		if (adminToken.length === 0) {
			setJoinTokenError("Admin token is missing.");
			return;
		}
		if (!nodesCapability.available || !runtime.isOnline) {
			setJoinTokenError(
				nodesCapability.reason ?? "The nodes API is unavailable.",
			);
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
			const message = formatBackendError(error);
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

		if (nodesCapability.unavailable && !hasQueryData(nodesState)) {
			return (
				<CapabilityUnavailableState
					title="Nodes unavailable"
					reason={nodesCapability.reason}
				/>
			);
		}

		if (nodesState.isLoading && !hasQueryData(nodesState)) {
			return (
				<PageState
					variant="loading"
					title="Loading nodes"
					description="Fetching nodes from the xp API."
				/>
			);
		}

		if (
			!hasQueryData(nodesState) &&
			queryIsOfflineBlocked(nodesState, runtime.isOnline)
		) {
			return (
				<PageState
					variant="offline"
					title="Offline cache unavailable"
					description="Open Nodes once while online to keep the latest cluster inventory available offline."
				/>
			);
		}
		if (nodesState.isError && !hasQueryData(nodesState)) {
			return (
				<PageState
					variant="error"
					title="Failed to load nodes"
					description={formatBackendError(nodesState.error)}
					error={nodesState.error}
					action={
						<Button
							variant="secondary"
							loading={nodesState.isFetching}
							onClick={() => nodesState.refetch()}
						>
							Retry
						</Button>
					}
				/>
			);
		}

		const nodes = nodesState.data?.items ?? [];
		if (nodes.length === 0) {
			return (
				<PageState
					variant="empty"
					title="No nodes yet"
					description="No nodes have been registered in this cluster."
					action={
						<Button
							variant="secondary"
							loading={nodesState.isFetching}
							onClick={() => nodesState.refetch()}
						>
							Refresh
						</Button>
					}
				/>
			);
		}

		return (
			<NodeInventoryList
				items={nodes}
				partial={nodesState.data?.partial ?? false}
				unreachableNodes={nodesState.data?.unreachable_nodes ?? []}
				isRefreshing={nodesState.isFetching}
				onRefresh={() => nodesState.refetch()}
			/>
		);
	})();
	const latestSyncedAt = latestQueryDataUpdatedAt([
		nodesState,
		clusterInfoState,
	]);
	const showCachedBanner =
		latestSyncedAt !== null &&
		(hasQueryData(nodesState) || hasQueryData(clusterInfoState)) &&
		(!runtime.isOnline || nodesState.isError || clusterInfoState.isError);
	const canCreateJoinToken =
		adminToken.length > 0 &&
		nodesCapability.available &&
		runtime.isOnline &&
		!runtime.isReadOnly;

	return (
		<div className="space-y-6">
			<PageHeader
				title="Nodes"
				description="Inspect cluster nodes and issue join tokens for new members."
			/>
			{showCachedBanner ? (
				<ReadStateBanner
					tone={!runtime.isOnline ? "warning" : "info"}
					variant="inline"
					dismissible
					errors={[nodesState.error, clusterInfoState.error]}
					title={
						!runtime.isOnline
							? "Offline node inventory"
							: "Showing cached node inventory"
					}
					description={`Last successful sync: ${formatSyncTimestamp(latestSyncedAt)}.`}
				/>
			) : null}

			<ModuleTabsLayout
				options={NODES_TAB_OPTIONS}
				value={nodesTabFromPath(location.pathname)}
				onValueChange={(value) => {
					if (value in NODES_TAB_PATHS) {
						navigate({ to: NODES_TAB_PATHS[value as NodesTab] });
					}
				}}
				ariaLabel="Nodes sections"
			>
				<ModuleTabsPanel value="nodes" keepMounted>
					<section className="space-y-4">
						<h2 className="text-lg font-semibold">Node inventory</h2>
						{nodesContent}
					</section>
				</ModuleTabsPanel>

				<ModuleTabsPanel value="join" keepMounted>
					<JoinNodePanel
						ttlSeconds={ttlSeconds}
						onTtlSecondsChange={setTtlSeconds}
						isCreatingJoinToken={isCreatingJoinToken}
						canCreateToken={canCreateJoinToken}
						onCreateJoinToken={handleCreateJoinToken}
						joinTokenError={joinTokenError}
						joinToken={joinToken}
						joinCommand={joinCommand}
						deployCommand={deployCommand}
					/>
				</ModuleTabsPanel>

				<ModuleTabsPanel value="repositories" keepMounted>
					<HistoryRepositoriesPanel
						adminToken={adminToken}
						nodes={nodesState.data?.items ?? []}
					/>
				</ModuleTabsPanel>
			</ModuleTabsLayout>
		</div>
	);
}
