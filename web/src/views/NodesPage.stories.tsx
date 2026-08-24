import type { Meta, StoryObj } from "@storybook/react";
import { expect, userEvent, within } from "@storybook/test";
import { fixtureCatalog } from "../fixture-policy/catalog";

import type { AdminNodesRuntimeResponse } from "../api/adminNodeRuntime";
import { BackendApiError } from "../api/backendError";
import { AppLayout } from "../components/AppLayout";
import { Button } from "../components/Button";
import { NodeInventoryList } from "../components/NodeInventoryList";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
import { ReadStateBanner } from "../components/ReadStateBanner";

const cachedNodesRuntime: AdminNodesRuntimeResponse = {
	partial: false,
	unreachable_nodes: [],
	items: [
		{
			node_id: fixtureCatalog.story.nodesPagePrimaryNodeId(),
			node_name: fixtureCatalog.story.nodesPagePrimaryNodeName(),
			api_base_url: fixtureCatalog.story.nodesPagePrimaryApiBaseUrl(),
			access_host: fixtureCatalog.story.nodesPagePrimaryAccessHost(),
			summary: {
				status: "up",
				updated_at: fixtureCatalog.timestamp.t20240101T035200(),
			},
			components: [
				{
					component: "xp",
					status: "up",
					consecutive_failures: 0,
					recoveries_observed: 2,
					restart_attempts: 0,
				},
			],
			recent_slots: [
				{
					slot_start: fixtureCatalog.timestamp.t20240101T035200(),
					status: "up",
				},
			],
		},
	],
};

const unauthorizedError = new BackendApiError({
	status: 401,
	code: "unauthorized",
	message: "missing or invalid authorization token",
});

function UnauthorizedNodesErrorPage() {
	return (
		<AppLayout>
			<div className="space-y-6">
				<PageHeader
					title="Nodes"
					description="Inspect cluster nodes and issue join tokens for new members."
				/>
				<section className="xp-card">
					<div className="xp-card-body">
						<PageState
							variant="error"
							title="Failed to load nodes"
							description="401 unauthorized: missing or invalid authorization token"
							error={unauthorizedError}
							action={<Button variant="secondary">Retry</Button>}
						/>
					</div>
				</section>
			</div>
		</AppLayout>
	);
}

function CachedUnauthorizedNodesPage() {
	return (
		<AppLayout>
			<div className="space-y-6">
				<PageHeader
					title="Nodes"
					description="Inspect cluster nodes and issue join tokens for new members."
				/>
				<ReadStateBanner
					tone="info"
					variant="inline"
					dismissible
					error={unauthorizedError}
					title="Showing cached node inventory"
					description="Last successful sync: 2026/7/30 15:00:00."
				/>
				<section className="xp-card">
					<div className="xp-card-body">
						<NodeInventoryList
							items={cachedNodesRuntime.items}
							partial={cachedNodesRuntime.partial}
							unreachableNodes={cachedNodesRuntime.unreachable_nodes}
						/>
					</div>
				</section>
			</div>
		</AppLayout>
	);
}

const meta = {
	title: "Pages/NodesPage",
	render: () => <div />,
	// These route-level stories are used for visual verification of the full app
	// shell and offline states. Exclude them from Storybook's interaction test
	// runner to keep CI focused on faster, deterministic coverage surfaces.
	tags: ["!test"],
	parameters: {
		router: {
			initialEntry: "/nodes",
		},
		mockApi: {
			data: {
				nodes: [
					{
						node_id: fixtureCatalog.story.nodesPagePrimaryNodeId(),
						node_name: fixtureCatalog.story.nodesPagePrimaryNodeName(),
						access_host: fixtureCatalog.story.nodesPagePrimaryAccessHost(),
						api_base_url: fixtureCatalog.story.nodesPagePrimaryApiBaseUrl(),
						quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
						quota_reset: fixtureCatalog.quota.resetNode(),
					},
					{
						node_id: fixtureCatalog.story.nodesPageSecondaryNodeId(),
						node_name: fixtureCatalog.story.nodesPageSecondaryNodeName(),
						access_host: fixtureCatalog.story.nodesPageSecondaryAccessHost(),
						api_base_url: fixtureCatalog.story.nodesPageSecondaryApiBaseUrl(),
						quota_limit_bytes: fixtureCatalog.quota.usedBytes(),
						quota_reset: fixtureCatalog.quota.resetNodeMidMonth(),
					},
				],
			},
		},
	},
} satisfies Meta;

export default meta;

type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const WithJoinToken: Story = {
	parameters: {
		router: {
			initialEntry: "/nodes/join",
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(
			canvas.getByRole("button", { name: /create token/i }),
		);
		await expect(
			await canvas.findByText(/xp-ops deploy command/i),
		).toBeInTheDocument();
		await expect(
			await canvas.findByText(/sudo xp-ops deploy/i),
		).toBeInTheDocument();
	},
};

export const HistoryRepositories: Story = {
	parameters: {
		router: {
			initialEntry: "/nodes/repositories",
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByRole("tab", { name: "历史仓库" })).toHaveAttribute(
			"aria-selected",
			"true",
		);
		await expect(await canvas.findByText("reachable")).toBeInTheDocument();
		await expect(
			await canvas.findByText(fixtureCatalog.story.nodesPagePrimaryNodeName()),
		).toBeInTheDocument();
		await expect(
			await canvas.findByText(fixtureCatalog.story.nodesPagePrimaryNodeId()),
		).toBeInTheDocument();
	},
};

export const Mobile393x852: Story = {
	parameters: {
		viewport: {
			defaultViewport: "nodesMobile393",
			viewports: {
				nodesMobile393: {
					name: "393x852",
					styles: { width: "393px", height: "852px" },
				},
			},
		},
	},
};

export const OfflineCachedInventory: Story = {
	parameters: {
		appRuntime: {
			initialIsOnline: false,
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			await canvas.findByText(/offline cached/i),
		).toBeInTheDocument();
		await expect(
			await canvas.findByText(/offline node inventory/i),
		).toBeInTheDocument();
		await expect(
			await canvas.findByText(/last successful sync:/i),
		).toBeInTheDocument();
	},
};

export const UnauthorizedLoadError: Story = {
	parameters: {
		router: {
			initialEntry: "/__story",
		},
		mockApi: {
			adminToken: "storybook-error-token",
		},
	},
	render: () => <UnauthorizedNodesErrorPage />,
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			await canvas.findByText("Failed to load nodes"),
		).toBeInTheDocument();
		await expect(
			canvas.getByText(
				"401 unauthorized: missing or invalid authorization token",
			),
		).toBeInTheDocument();
		await expect(
			canvas.getByRole("link", { name: "Sign in" }),
		).toBeInTheDocument();
		await expect(
			canvas.getByRole("button", { name: "Retry" }),
		).toBeInTheDocument();
	},
};

export const CachedUnauthorizedInventory: Story = {
	parameters: {
		router: {
			initialEntry: "/__story",
		},
	},
	render: () => <CachedUnauthorizedNodesPage />,
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			await canvas.findByText("Showing cached node inventory"),
		).toBeInTheDocument();
		const signInAgain = await canvas.findByRole("link", {
			name: "Sign in",
		});
		await expect(signInAgain).toHaveAttribute(
			"href",
			expect.stringContaining("/login?redirect="),
		);
		await expect(
			canvas.getByText(fixtureCatalog.story.nodesPagePrimaryNodeName()),
		).toBeInTheDocument();
	},
};
