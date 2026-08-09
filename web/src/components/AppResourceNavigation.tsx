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
	groups: AppNavigationGroup[];
	pathname: string;
	onNavigate: (href: string) => void;
	onResourceNavigate: (href: string) => void;
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
	groups,
	pathname,
	onNavigate,
	onResourceNavigate,
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
	const adminNodes = useQuery({
		queryKey: ["adminNodes", adminToken],
		enabled:
			adminToken.length > 0 &&
			nodesCapability.available &&
			(requestedResources.nodes || nodesActive),
		queryFn: ({ signal }) => fetchAdminNodes(adminToken, signal),
	});
	const adminEndpoints = useQuery({
		queryKey: ["adminEndpoints", adminToken],
		enabled:
			adminToken.length > 0 &&
			endpointsCapability.available &&
			(requestedResources.endpoints || endpointsActive),
		queryFn: ({ signal }) => fetchAdminEndpoints(adminToken, signal),
	});
	const adminUsers = useQuery({
		queryKey: ["adminUsers", adminToken],
		enabled:
			adminToken.length > 0 &&
			usersCapability.available &&
			(requestedResources.users || usersActive),
		queryFn: ({ signal }) => fetchAdminUsers(adminToken, signal),
	});

	const resourceGroups = useMemo<ResourceNavigationGroup[]>(() => {
		const resourceItems = {
			nodes: {
				children: (adminNodes.data?.items ?? []).map((node) => ({
					id: node.node_id,
					label: node.node_name || "Unnamed node",
					href: `/nodes/${encodeURIComponent(node.node_id)}`,
					ariaLabel: `Node ${node.node_name || "unnamed"} (${node.node_id})`,
				})),
				isLoading:
					adminNodes.isLoading ||
					((requestedResources.nodes || nodesActive) &&
						nodesCapability.pending),
				error: adminNodes.isError
					? formatApiError(adminNodes.error)
					: adminToken.length === 0 && (requestedResources.nodes || nodesActive)
						? "Admin token required"
						: nodesCapability.unavailable
							? nodesCapability.reason
							: null,
				onRetry: () => void adminNodes.refetch(),
			},
			endpoints: {
				children: (adminEndpoints.data?.items ?? []).map((endpoint) => ({
					id: endpoint.endpoint_id,
					label: endpoint.tag || "Untitled endpoint",
					href: `/endpoints/${encodeURIComponent(endpoint.endpoint_id)}`,
					ariaLabel: `Endpoint ${endpoint.tag || "untitled"} (${endpoint.endpoint_id})`,
				})),
				isLoading:
					adminEndpoints.isLoading ||
					((requestedResources.endpoints || endpointsActive) &&
						endpointsCapability.pending),
				error: adminEndpoints.isError
					? formatApiError(adminEndpoints.error)
					: adminToken.length === 0 &&
							(requestedResources.endpoints || endpointsActive)
						? "Admin token required"
						: endpointsCapability.unavailable
							? endpointsCapability.reason
							: null,
				onRetry: () => void adminEndpoints.refetch(),
			},
			users: {
				children: (adminUsers.data?.items ?? []).map((user) => ({
					id: user.user_id,
					label: user.display_name || "Unnamed user",
					href: `/users/${encodeURIComponent(user.user_id)}`,
					ariaLabel: `User ${user.display_name || "unnamed"} (${user.user_id})`,
				})),
				isLoading:
					adminUsers.isLoading ||
					((requestedResources.users || usersActive) &&
						usersCapability.pending),
				error: adminUsers.isError
					? formatApiError(adminUsers.error)
					: adminToken.length === 0 && (requestedResources.users || usersActive)
						? "Admin token required"
						: usersCapability.unavailable
							? usersCapability.reason
							: null,
				onRetry: () => void adminUsers.refetch(),
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
		endpointsCapability,
		groups,
		nodesCapability,
		nodesActive,
		requestedResources,
		usersCapability,
		endpointsActive,
		usersActive,
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
