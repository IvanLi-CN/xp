import type { Meta, StoryObj } from "@storybook/react";
import { expect, userEvent, within } from "@storybook/test";

import type { AdminNodesRuntimeResponse } from "../api/adminNodeRuntime";
import { BackendApiError } from "../api/backendError";
import { AppLayout } from "../components/AppLayout";
import { NodeInventoryList } from "../components/NodeInventoryList";
import { PageHeader } from "../components/PageHeader";
import { ReadStateBanner } from "../components/ReadStateBanner";

const cachedNodesRuntime: AdminNodesRuntimeResponse = {
	partial: false,
	unreachable_nodes: [],
	items: [
		{
			node_id: "01J000000000000000000000001",
			node_name: "node-a",
			api_base_url: "https://node-a.example.invalid",
			access_host: "node-a.example.invalid",
			summary: {
				status: "up",
				updated_at: "2026-07-30T06:00:00.000Z",
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
					slot_start: "2026-07-30T06:00:00.000Z",
					status: "up",
				},
			],
		},
	],
};

const cachedUnauthorizedError = new BackendApiError({
	status: 401,
	code: "unauthorized",
	message: "missing or invalid authorization token",
});

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
					error={cachedUnauthorizedError}
					title="Showing cached node inventory"
					description="Last successful sync: 2026/7/30 15:00:00."
				/>
				<section className="xp-card">
					<div className="xp-card-body space-y-4">
						<h2 className="xp-card-title">Node inventory</h2>
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
						node_id: "01J000000000000000000000001",
						node_name: "node-a",
						access_host: "node-a.example.invalid",
						api_base_url: "https://node-a.example.invalid",
						quota_limit_bytes: 0,
						quota_reset: {
							policy: "monthly",
							day_of_month: 1,
							tz_offset_minutes: null,
						},
					},
					{
						node_id: "01J000000000000000000000002",
						node_name: "node-b",
						access_host: "node-b.example.invalid",
						api_base_url: "https://node-b.example.invalid",
						quota_limit_bytes: 0,
						quota_reset: {
							policy: "monthly",
							day_of_month: 15,
							tz_offset_minutes: null,
						},
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
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			await canvas.findAllByRole("link", { name: "Details" }),
		).toHaveLength(2);
		const openLinks = await canvas.findAllByRole("link", {
			name: "Open on node",
		});
		await expect(openLinks).toHaveLength(2);
		await expect(openLinks[0]).toHaveAttribute(
			"href",
			"https://node-a.example.invalid/iframe.html?id=pages-nodespage--with-join-token&viewMode=%7B%7BviewMode%7D%7D&login_token=storybook-admin-token",
		);
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
			name: "Sign in again",
		});
		await expect(signInAgain).toHaveAttribute(
			"href",
			expect.stringContaining("/login?redirect="),
		);
		await expect(canvas.getByText("node-a")).toBeInTheDocument();
	},
};
