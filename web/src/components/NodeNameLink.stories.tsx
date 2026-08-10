import type { Meta, StoryObj } from "@storybook/react";
import { expect, within } from "@storybook/test";
import { fixtureCatalog } from "../fixture-policy/catalog";

import { NodeNameLink } from "./NodeNameLink";

const meta = {
	title: "Components/NodeNameLink",
	component: NodeNameLink,
	tags: ["autodocs", "coverage-ui"],
	decorators: [
		(Story) => (
			<div className="w-full max-w-none space-y-4 rounded-lg border border-white/30 bg-card p-6">
				<Story />
			</div>
		),
	],
} satisfies Meta<typeof NodeNameLink>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Resolved: Story = {
	args: {
		nodeId: fixtureCatalog.nodeId.fixture290(),
		nodeName: fixtureCatalog.nodeName.fixture291(),
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const link = await canvas.findByRole("link", {
			name: new RegExp(fixtureCatalog.nodeName.fixture291()),
		});
		expect(link).toHaveAttribute(
			"href",
			`/nodes/${fixtureCatalog.nodeId.fixture290()}`,
		);
		expect(link).toHaveAttribute("title", fixtureCatalog.nodeId.fixture290());
	},
};

export const LongName: Story = {
	args: {
		nodeId: fixtureCatalog.nodeId.fixture290(),
		nodeName: fixtureCatalog.nodeName.fixture292(),
	},
};

export const NameUnavailable: Story = {
	args: {
		nodeId: fixtureCatalog.nodeId.fixture290(),
		nodeName: fixtureCatalog.string.none(),
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		expect(canvas.queryByRole("link")).toBeNull();
		expect(
			await canvas.findByText(fixtureCatalog.nodeId.fixture290()),
		).toBeInTheDocument();
	},
};

export const StateMatrix: Story = {
	args: {
		nodeId: fixtureCatalog.nodeId.fixture290(),
		nodeName: fixtureCatalog.nodeName.fixture291(),
	},
	render: () => (
		<div className="flex min-h-[720px] flex-col justify-center gap-10">
			<div className="space-y-2">
				<p className="text-sm text-muted-foreground">Resolved</p>
				<NodeNameLink
					nodeId={fixtureCatalog.nodeId.fixture290()}
					nodeName={fixtureCatalog.nodeName.fixture291()}
				/>
			</div>
			<div className="space-y-2">
				<p className="text-sm text-muted-foreground">Long name</p>
				<NodeNameLink
					nodeId={fixtureCatalog.nodeId.fixture290()}
					nodeName={fixtureCatalog.nodeName.fixture292()}
				/>
			</div>
			<div className="space-y-2">
				<p className="text-sm text-muted-foreground">Name unavailable</p>
				<NodeNameLink
					nodeId={fixtureCatalog.nodeId.fixture290()}
					nodeName={fixtureCatalog.string.none()}
				/>
			</div>
		</div>
	),
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		expect(await canvas.findAllByRole("link")).toHaveLength(2);
		expect(
			await canvas.findByText(fixtureCatalog.nodeId.fixture290()),
		).toBeInTheDocument();
	},
};
