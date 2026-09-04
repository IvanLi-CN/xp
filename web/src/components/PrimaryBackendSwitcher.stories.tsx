import type { Meta, StoryObj } from "@storybook/react";

import { PrimaryBackendProvider } from "@/backend/PrimaryBackendProvider";
import {
	type PrimaryBackendSnapshot,
	getPrimaryBackendSnapshot,
	hydratePrimaryBackendProfile,
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
	},
	decorators: [
		(Story) => {
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

function withSnapshot(snapshot: PrimaryBackendSnapshot): Story["render"] {
	return (args) => (
		<PrimaryBackendProvider snapshot={snapshot}>
			<PrimaryBackendSwitcher {...args} />
		</PrimaryBackendProvider>
	);
}

export const Default: Story = {
	render: (args) => {
		hydratePrimaryBackendProfile("cluster-demo", [
			{
				node_id: fixtureCatalog.identifier.nodePrimary(),
				node_name: fixtureCatalog.identifier.nodeNamePrimary(),
				api_base_url: fixtureCatalog.url.primaryApi(),
			},
			{
				node_id: fixtureCatalog.identifier.nodeSecondary(),
				node_name: fixtureCatalog.identifier.nodeNameSecondary(),
				api_base_url: fixtureCatalog.url.secondaryApi(),
			},
		]);
		return (
			<PrimaryBackendProvider>
				<PrimaryBackendSwitcher {...args} />
			</PrimaryBackendProvider>
		);
	},
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
