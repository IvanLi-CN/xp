import type { Meta, StoryObj } from "@storybook/react";
import { expect, fn, within } from "@storybook/test";

import { demoMeshStatus } from "@/demo/DemoSystemStatusPage";

import { MeshClusterSetting } from "./MeshClusterSetting";

const baseArgs = {
	status: { ...demoMeshStatus, cluster_mesh_enabled: true },
	statusUpdatedAt: Date.now(),
	statusFresh: true,
	observerBackend: "https://primary.example",
	adminTokenPresent: true,
	capabilityAvailable: true,
	isOnline: true,
	isReadOnly: false,
	mutationPhase: "idle" as const,
	pendingTarget: null,
	onRequestChange: fn(),
	onConfirm: fn(),
	onCancel: fn(),
	onRefresh: fn(),
	onUnlock: fn(),
};

const meta = {
	title: "Components/MeshClusterSetting",
	component: MeshClusterSetting,
	render: (args) => (
		<div
			data-visual-evidence-surface
			className="bg-background p-6 text-foreground"
		>
			<div data-visual-evidence-target className="mx-auto max-w-6xl">
				<MeshClusterSetting {...args} />
			</div>
		</div>
	),
	tags: ["autodocs", "coverage-ui"],
	parameters: {
		docs: {
			description: {
				component:
					"Cluster-wide Reality Direct Mesh control with explicit confirmation, " +
					"local unknown-result recovery, and observer-only peer path facts.",
			},
		},
	},
	args: baseArgs,
} satisfies Meta<typeof MeshClusterSetting>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Enabled: Story = {};

export const Disabled: Story = {
	args: {
		status: { ...demoMeshStatus, cluster_mesh_enabled: false },
	},
};

export const UnknownOffline: Story = {
	args: {
		status: { ...demoMeshStatus, cluster_mesh_enabled: undefined },
		statusUpdatedAt: Date.now() - 90_000,
		statusFresh: false,
		isOnline: false,
		isReadOnly: true,
	},
	parameters: {
		viewport: {
			defaultViewport: "meshMobile393",
			viewports: {
				meshMobile393: {
					name: "393x852",
					styles: { width: "393px", height: "852px" },
				},
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("switch", { name: "Enable Reality Direct Mesh" }),
		).toBeDisabled();
		await expect(canvas.getAllByText("Unknown", { exact: true })).toHaveLength(
			2,
		);
	},
};

export const PreflightFailure: Story = {
	args: {
		status: { ...demoMeshStatus, cluster_mesh_enabled: false },
		mutationIssue: {
			kind: "preflight",
			message:
				"Directed cluster path validation failed. No setting change was confirmed.",
		},
		preflightFailures: [
			{
				sender_node_id: demoMeshStatus.local.node_id,
				target_node_id: demoMeshStatus.peers[0]?.node_id ?? "peer-unknown",
				kind: "protocol",
			},
		],
	},
};

export const ConfirmEnable: Story = {
	args: {
		status: { ...demoMeshStatus, cluster_mesh_enabled: false },
		pendingTarget: true,
		currentConfirmationValue: false,
		mutationPhase: "confirming",
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const body = within(canvasElement.ownerDocument.body);
		await expect(
			await body.findByRole("alertdialog", {
				name: "Enable Reality Direct Mesh?",
			}),
		).toBeInTheDocument();
		await expect(canvas.getByText(/directed voter paths/)).toBeInTheDocument();
	},
};

export const UnknownResultLocked: Story = {
	args: {
		unknownLock: {
			clusterId: demoMeshStatus.local.cluster_id,
			targetEnabled: true,
			createdAt: Date.now() - 5_000,
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByText("Mutation result unknown"),
		).toBeInTheDocument();
		await expect(
			canvas.getByRole("button", { name: "Verify and unlock" }),
		).toBeInTheDocument();
		await expect(
			canvas.getByRole("switch", { name: "Enable Reality Direct Mesh" }),
		).toBeDisabled();
	},
};
