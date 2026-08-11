import { QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { resolveApiCompatibility } from "../api/apiCompatibility";
import { fixtureCatalog } from "../fixture-policy/catalog";
import { createQueryClient } from "../queryClient";
import { AppResourceNavigation } from "./AppResourceNavigation";

const groups = [
	{
		title: "Nav",
		items: [{ label: "Users", to: "/users", icon: "tabler:users" }],
	},
];

const nodeGroups = [
	{
		title: "Nav",
		items: [{ label: "Nodes", to: "/nodes", icon: "tabler:server" }],
	},
];

function formatNodeLabel(nodeName: string, nodeId: string) {
	return `${nodeName} (${nodeId})`;
}

describe("<AppResourceNavigation />", () => {
	it("shows a retryable compatibility error instead of permanent loading", () => {
		const onRetryCompatibility = vi.fn();
		render(
			<QueryClientProvider client={createQueryClient()}>
				<AppResourceNavigation
					adminToken="admintoken"
					compatibility={null}
					compatibilityError="Compatibility request failed"
					compatibilityPending={false}
					groups={groups}
					localNodeId={null}
					pathname="/users"
					onNavigate={vi.fn()}
					onResourceNavigate={vi.fn()}
					onRetryCompatibility={onRetryCompatibility}
				/>
			</QueryClientProvider>,
		);

		fireEvent.click(screen.getByRole("button", { name: "Expand Users" }));
		expect(screen.queryByText("Loading users...")).toBeNull();
		expect(screen.getByText("Unable to load")).toBeInTheDocument();

		fireEvent.click(screen.getByRole("button", { name: "Retry Users" }));
		expect(onRetryCompatibility).toHaveBeenCalledTimes(1);
	});

	it("marks only the cluster-info node as the current hosting node", () => {
		const queryClient = createQueryClient();
		queryClient.setQueryData(["adminNodes", "admintoken"], {
			items: [
				{
					node_id: fixtureCatalog.identifier.nodePrimary(),
					node_name: fixtureCatalog.identifier.nodeNamePrimary(),
					api_base_url: fixtureCatalog.url.primaryApi(),
					access_host: fixtureCatalog.host.primary(),
					quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
					quota_reset: fixtureCatalog.quota.reset(),
				},
				{
					node_id: fixtureCatalog.identifier.nodeSecondary(),
					node_name: fixtureCatalog.identifier.nodeNameSecondary(),
					api_base_url: fixtureCatalog.url.secondaryApi(),
					access_host: fixtureCatalog.host.secondary(),
					quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
					quota_reset: fixtureCatalog.quota.reset(),
				},
			],
		});
		const compatibility = resolveApiCompatibility({
			capabilities: ["admin.nodes"],
		});

		render(
			<QueryClientProvider client={queryClient}>
				<AppResourceNavigation
					adminToken="admintoken"
					compatibility={compatibility}
					compatibilityError={null}
					compatibilityPending={false}
					groups={nodeGroups}
					localNodeId={fixtureCatalog.identifier.nodePrimary()}
					pathname={`/nodes/${fixtureCatalog.identifier.nodeSecondary()}`}
					onNavigate={vi.fn()}
					onResourceNavigate={vi.fn()}
					onRetryCompatibility={vi.fn()}
				/>
			</QueryClientProvider>,
		);

		const primaryNodeLabel = formatNodeLabel(
			fixtureCatalog.identifier.nodeNamePrimary(),
			fixtureCatalog.identifier.nodePrimary(),
		);
		const secondaryNodeLabel = formatNodeLabel(
			fixtureCatalog.identifier.nodeNameSecondary(),
			fixtureCatalog.identifier.nodeSecondary(),
		);
		const currentNode = screen.getByRole("link", {
			name: `Current hosting node ${primaryNodeLabel}`,
		});
		const ordinaryNode = screen.getByRole("link", {
			name: `Node ${secondaryNodeLabel}`,
		});
		expect(currentNode).toHaveAttribute(
			"data-leading-icon-name",
			"tabler:server-bolt",
		);
		expect(currentNode).toHaveAttribute("data-leading-icon-tone", "primary");
		expect(ordinaryNode).toHaveAttribute(
			"data-leading-icon-name",
			"tabler:server",
		);
		expect(ordinaryNode).toHaveAttribute("data-leading-icon-tone", "muted");
	});
});
