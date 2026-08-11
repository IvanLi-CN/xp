import { useQuery } from "@tanstack/react-query";
import { useMemo, useState } from "react";

import { fetchAdminEndpoints } from "../api/adminEndpoints";
import { fetchAdminNodes } from "../api/adminNodes";
import { fetchAdminUsers } from "../api/adminUsers";
import type { ApiCompatibilityResult } from "../api/apiCompatibility";
import { isBackendApiError } from "../api/backendError";
import { getApiCapabilityState } from "../api/useApiCompatibility";
import {
	ResourceNavigation,
	type ResourceNavigationGroup,
} from "./ResourceNavigation";

export type AppNavigationGroup = {
	title: string;
	items: Array<{ label: string; to: string; icon: string }>;
};

type ResourceId = "nodes" | "endpoints" | "users";

type AppResourceNavigationProps = {
	adminToken: string;
	compatibility: ApiCompatibilityResult | null;
	compatibilityError: string | null;
	compatibilityPending: boolean;
	groups: AppNavigationGroup[];
	localNodeId: string | null;
	pathname: string;
	onNavigate: (href: string) => void;
	onResourceNavigate: (href: string) => void;
	onRetryCompatibility: () => void;
};

function formatApiError(error: unknown): string {
	if (isBackendApiError(error)) {
		return error.code
			? `${error.status} ${error.code}: ${error.message}`
			: `${error.status}: ${error.message}`;
	}
	return error instanceof Error ? error.message : "request failed";
}

function resourceIdForPath(path: string): ResourceId | null {
	if (path === "/nodes") return "nodes";
	if (path === "/endpoints") return "endpoints";
	if (path === "/users") return "users";
	return null;
}

export function AppResourceNavigation({
	adminToken,
	compatibility,
	compatibilityError,
	compatibilityPending,
	groups,
	localNodeId,
	pathname,
	onNavigate,
	onResourceNavigate,
	onRetryCompatibility,
}: AppResourceNavigationProps) {
	const [requestedResources, setRequestedResources] = useState<
		Record<ResourceId, boolean>
	>({ nodes: false, endpoints: false, users: false });
	const nodesCapability = getApiCapabilityState(compatibility, "admin.nodes");
	const endpointsCapability = getApiCapabilityState(
		compatibility,
		"admin.endpoints",
	);
	const usersCapability = getApiCapabilityState(compatibility, "admin.users");
	const nodesActive = pathname.startsWith("/nodes/");
	const endpointsActive = pathname.startsWith("/endpoints/");
	const usersActive = pathname.startsWith("/users/");
	const nodesRequested = requestedResources.nodes || nodesActive;
	const endpointsRequested = requestedResources.endpoints || endpointsActive;
	const usersRequested = requestedResources.users || usersActive;
	const adminNodes = useQuery({
		queryKey: ["adminNodes", adminToken],
		enabled:
			adminToken.length > 0 && nodesCapability.available && nodesRequested,
		queryFn: ({ signal }) => fetchAdminNodes(adminToken, signal),
	});
	const adminEndpoints = useQuery({
		queryKey: ["adminEndpoints", adminToken],
		enabled:
			adminToken.length > 0 &&
			endpointsCapability.available &&
			endpointsRequested,
		queryFn: ({ signal }) => fetchAdminEndpoints(adminToken, signal),
	});
	const adminUsers = useQuery({
		queryKey: ["adminUsers", adminToken],
		enabled:
			adminToken.length > 0 && usersCapability.available && usersRequested,
		queryFn: ({ signal }) => fetchAdminUsers(adminToken, signal),
	});

	const resourceGroups = useMemo<ResourceNavigationGroup[]>(() => {
		const resourceItems = {
			nodes: {
				children: (adminNodes.data?.items ?? []).map((node) => {
					const isLocalNode = node.node_id === localNodeId;
					const nodeName = node.node_name || "Unnamed node";
					const identityLabel = isLocalNode ? "Current hosting node" : "Node";
					return {
						id: node.node_id,
						label: nodeName,
						href: `/nodes/${encodeURIComponent(node.node_id)}`,
						ariaLabel: `${identityLabel} ${nodeName} (${node.node_id})`,
						leadingIcon: {
							name: isLocalNode ? "tabler:server-bolt" : "tabler:server-2",
							tone: isLocalNode ? "primary" : "muted",
						} as const,
					};
				}),
				isLoading:
					adminNodes.isLoading || (nodesRequested && compatibilityPending),
				error: adminNodes.isError
					? formatApiError(adminNodes.error)
					: compatibilityError && nodesRequested
						? compatibilityError
						: adminToken.length === 0 && nodesRequested
							? "Admin token required"
							: nodesCapability.unavailable
								? nodesCapability.reason
								: null,
				onRetry: () =>
					compatibilityError
						? onRetryCompatibility()
						: void adminNodes.refetch(),
			},
			endpoints: {
				children: (adminEndpoints.data?.items ?? []).map((endpoint) => ({
					id: endpoint.endpoint_id,
					label: endpoint.tag || "Untitled endpoint",
					href: `/endpoints/${encodeURIComponent(endpoint.endpoint_id)}`,
					ariaLabel: `Endpoint ${endpoint.tag || "untitled"} (${endpoint.endpoint_id})`,
					leadingIcon: {
						name: "tabler:link",
						tone: "muted",
					} as const,
				})),
				isLoading:
					adminEndpoints.isLoading ||
					(endpointsRequested && compatibilityPending),
				error: adminEndpoints.isError
					? formatApiError(adminEndpoints.error)
					: compatibilityError && endpointsRequested
						? compatibilityError
						: adminToken.length === 0 && endpointsRequested
							? "Admin token required"
							: endpointsCapability.unavailable
								? endpointsCapability.reason
								: null,
				onRetry: () =>
					compatibilityError
						? onRetryCompatibility()
						: void adminEndpoints.refetch(),
			},
			users: {
				children: (adminUsers.data?.items ?? []).map((user) => ({
					id: user.user_id,
					label: user.display_name || "Unnamed user",
					href: `/users/${encodeURIComponent(user.user_id)}`,
					ariaLabel: `User ${user.display_name || "unnamed"} (${user.user_id})`,
					leadingIcon: {
						name: "tabler:user-circle",
						tone: "muted",
					} as const,
				})),
				isLoading:
					adminUsers.isLoading || (usersRequested && compatibilityPending),
				error: adminUsers.isError
					? formatApiError(adminUsers.error)
					: compatibilityError && usersRequested
						? compatibilityError
						: adminToken.length === 0 && usersRequested
							? "Admin token required"
							: usersCapability.unavailable
								? usersCapability.reason
								: null,
				onRetry: () =>
					compatibilityError
						? onRetryCompatibility()
						: void adminUsers.refetch(),
			},
		};
		return groups.map((group) => ({
			title: group.title,
			items: group.items.map((item) => {
				const resourceId = resourceIdForPath(item.to);
				return {
					id: resourceId ?? item.to,
					label: item.label,
					href: item.to,
					icon: item.icon,
					...(resourceId ? resourceItems[resourceId] : {}),
				};
			}),
		}));
	}, [
		adminEndpoints,
		adminNodes,
		adminToken.length,
		adminUsers,
		compatibilityError,
		compatibilityPending,
		endpointsCapability,
		endpointsRequested,
		groups,
		localNodeId,
		nodesCapability,
		nodesRequested,
		onRetryCompatibility,
		usersCapability,
		usersRequested,
	]);

	return (
		<ResourceNavigation
			ariaLabel="Primary navigation"
			groups={resourceGroups}
			pathname={pathname}
			onNavigate={onNavigate}
			onResourceNavigate={onResourceNavigate}
			onResourceRequested={(resourceId) => {
				if (resourceIdForPath(`/${resourceId}`) === null) return;
				setRequestedResources((current) => ({
					...current,
					[resourceId]: true,
				}));
			}}
		/>
	);
}
