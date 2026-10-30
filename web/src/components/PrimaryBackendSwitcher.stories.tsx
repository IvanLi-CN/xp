import type { Meta, StoryObj } from "@storybook/react";
import { expect, fn, screen, userEvent, within } from "@storybook/test";
import { useState } from "react";

import { PrimaryBackendProvider } from "@/backend/PrimaryBackendProvider";
import {
	type PrimaryBackendSnapshot,
	getPrimaryBackendSnapshot,
} from "@/backend/primaryBackend";
import { fixtureCatalog } from "@/fixture-policy/catalog";

import { PrimaryBackendSwitcher } from "./PrimaryBackendSwitcher";

const meta: Meta<typeof PrimaryBackendSwitcher> = {
	title: "Components/PrimaryBackendSwitcher",
	component: PrimaryBackendSwitcher,
	tags: ["autodocs", "coverage-ui"],
	args: {
		adminToken: "test-token",
		clusterId: "cluster-demo",
		onOpenChange: fn(),
		onOpened: fn(),
		onSwitched: fn(),
	},
	decorators: [
		(Story, context) => {
			const [target, setTarget] = useState<HTMLElement | null>(null);
			if (context.parameters.portalEvidence) {
				return (
					<div
						data-visual-evidence-surface
						className="w-fit bg-background p-6 text-foreground"
					>
						<div
							data-visual-evidence-target
							ref={setTarget}
							className="flex h-[360px] w-[344px] justify-end"
						>
							<Story args={{ ...context.args, portalContainer: target }} />
						</div>
					</div>
				);
			}
			return (
				<div
					data-visual-evidence-surface
					className="min-h-24 bg-background p-6 text-foreground"
				>
					<div data-visual-evidence-target>
						<Story />
					</div>
				</div>
			);
		},
	],
};

export default meta;

type Story = StoryObj<typeof PrimaryBackendSwitcher>;

const candidates = [
	{
		origin: fixtureCatalog.url.primaryApi(),
		nodeId: fixtureCatalog.identifier.nodePrimary(),
		nodeName: fixtureCatalog.identifier.nodeNamePrimary(),
		verifiedAt: Date.now(),
		lastError: null,
	},
	{
		origin: fixtureCatalog.url.secondaryApi(),
		nodeId: fixtureCatalog.identifier.nodeSecondary(),
		nodeName: fixtureCatalog.identifier.nodeNameSecondary(),
		verifiedAt: Date.now(),
		lastError: null,
	},
];

const versionedCandidates = [
	{
		origin: fixtureCatalog.url.primaryApi(),
		nodeId: fixtureCatalog.identifier.nodePrimary(),
		nodeName: fixtureCatalog.identifier.nodeNamePrimary(),
		verifiedAt: Date.now(),
		lastError: null,
		switchable: true,
		serviceVersion: "3.22.5",
		versionState: "ready" as const,
	},
	{
		origin: fixtureCatalog.url.secondaryApi(),
		nodeId: fixtureCatalog.identifier.nodeSecondary(),
		nodeName: fixtureCatalog.identifier.nodeNameSecondary(),
		verifiedAt: Date.now(),
		lastError: null,
		switchable: true,
		serviceVersion: "3.21.11",
		versionState: "ready" as const,
	},
	{
		origin: "https://legacy.xp.example",
		nodeId: fixtureCatalog.nodeId.fixture17(),
		nodeName: fixtureCatalog.nodeName.fixture18(),
		verifiedAt: null,
		lastError: "unreachable",
		switchable: false,
		switchBlockedReason: "This node is not available to the static console.",
		serviceVersion: null,
		versionState: "unavailable" as const,
	},
	{
		origin: "http://insecure.xp.example",
		nodeId: fixtureCatalog.nodeId.fixture32(),
		nodeName: fixtureCatalog.nodeName.fixture33(),
		verifiedAt: null,
		lastError: "invalid_origin",
		switchable: false,
		switchBlockedReason: "HTTPS is required for this node.",
	},
];

function withSnapshot(snapshot: PrimaryBackendSnapshot): Story["render"] {
	return (args) => (
		<PrimaryBackendProvider snapshot={snapshot}>
			<PrimaryBackendSwitcher {...args} />
		</PrimaryBackendProvider>
	);
}

export const Default: Story = {
	render: withSnapshot({
		...getPrimaryBackendSnapshot(),
		clusterId: "cluster-demo",
		primaryOrigin: window.location.origin,
		candidates,
	}),
};

export const SignedOut: Story = {
	args: { adminToken: "", clusterId: null },
	render: withSnapshot({
		...getPrimaryBackendSnapshot(),
		clusterId: null,
		primaryOrigin: window.location.origin,
		candidates: [],
	}),
};

export const Unreachable: Story = {
	render: withSnapshot({
		...getPrimaryBackendSnapshot(),
		clusterId: "cluster-demo",
		primaryOrigin: window.location.origin,
		candidates,
		state: "unreachable",
	}),
};

export const MutationBlocked: Story = {
	render: withSnapshot({
		...getPrimaryBackendSnapshot(),
		clusterId: "cluster-demo",
		primaryOrigin: window.location.origin,
		candidates,
		state: "switching",
		pendingMutations: 1,
	}),
};

export const VersionMismatchAndLinks: Story = {
	args: { frontendVersion: "3.22.5" },
	render: withSnapshot({
		...getPrimaryBackendSnapshot(),
		clusterId: "cluster-demo",
		primaryOrigin: fixtureCatalog.url.primaryApi(),
		candidates: versionedCandidates,
		state: "ready",
	}),
};

export const StaleDirectory: Story = {
	args: { frontendVersion: "3.22.5", directoryStatus: "stale" },
	parameters: { portalEvidence: true },
	render: withSnapshot({
		...getPrimaryBackendSnapshot(),
		clusterId: "cluster-demo",
		primaryOrigin: fixtureCatalog.url.primaryApi(),
		candidates: versionedCandidates,
		state: "ready",
	}),
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(
			canvas.getByRole("button", { name: "Open primary backend" }),
		);
		await expect(canvas.getByRole("status")).toHaveTextContent(
			"Node list may be out of date.",
		);
		await expect(
			canvas.getByText(fixtureCatalog.identifier.nodeNamePrimary()),
		).toBeVisible();
	},
};

export const DirectoryUnavailable: Story = {
	args: { directoryStatus: "error" },
	render: withSnapshot({
		...getPrimaryBackendSnapshot(),
		clusterId: "cluster-demo",
		primaryOrigin: fixtureCatalog.url.primaryApi(),
		candidates: [],
		state: "unreachable",
	}),
};

export const UnrecognizedFrontendVersion: Story = {
	args: { frontendVersion: "3.22.5-buildhash" },
	render: withSnapshot({
		...getPrimaryBackendSnapshot(),
		clusterId: "cluster-demo",
		primaryOrigin: fixtureCatalog.url.primaryApi(),
		candidates: versionedCandidates,
		state: "ready",
	}),
};

export const StagedReload: Story = {
	tags: ["alert-reload"],
	args: { onOpened: fn(), onOpenChange: fn(), onSwitched: fn() },
	render: withSnapshot({
		...getPrimaryBackendSnapshot(),
		clusterId: "cluster-demo",
		primaryOrigin: fixtureCatalog.url.primaryApi(),
		candidates: [
			candidates[0],
			{
				...candidates[1],
				runtimeState: "staged",
				runtimePolicyId: "story-policy",
			},
		],
		state: "ready",
	}),
	play: async () => {
		await userEvent.click(
			screen.getByRole("button", { name: "Open primary backend" }),
		);
		await userEvent.click(
			screen.getByRole("button", { name: candidates[1].nodeName }),
		);
		await expect(
			screen.getByRole("button", { name: "Reload once" }),
		).toBeVisible();
	},
};
