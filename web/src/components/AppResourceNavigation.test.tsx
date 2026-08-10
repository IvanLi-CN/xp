import { QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { resolveApiCompatibility } from "../api/apiCompatibility";
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
					node_id: "node-tokyo-1",
					node_name: "Tokyo edge",
					api_base_url: "https://tokyo.example.test",
					access_host: "tokyo.example.test",
					quota_limit_bytes: 0,
					quota_reset: { policy: "unlimited" },
				},
				{
					node_id: "node-osaka-1",
					node_name: "Osaka edge",
					api_base_url: "https://osaka.example.test",
					access_host: "osaka.example.test",
					quota_limit_bytes: 0,
					quota_reset: { policy: "unlimited" },
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
					localNodeId="node-tokyo-1"
					pathname="/nodes/node-osaka-1"
					onNavigate={vi.fn()}
					onResourceNavigate={vi.fn()}
					onRetryCompatibility={vi.fn()}
				/>
			</QueryClientProvider>,
		);

		const currentNode = screen.getByRole("link", {
			name: "Current hosting node Tokyo edge (node-tokyo-1)",
		});
		const ordinaryNode = screen.getByRole("link", {
			name: "Node Osaka edge (node-osaka-1)",
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
