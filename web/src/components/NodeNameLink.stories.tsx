import type { Meta, StoryObj } from "@storybook/react";
import { expect, within } from "@storybook/test";

import { NodeNameLink } from "./NodeNameLink";

const meta = {
	title: "Components/NodeNameLink",
	component: NodeNameLink,
	tags: ["autodocs", "coverage-ui"],
	decorators: [
		(Story) => (
			<div className="w-full max-w-none space-y-4 rounded-lg border border-border bg-card p-6">
				<Story />
			</div>
		),
	],
} satisfies Meta<typeof NodeNameLink>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Resolved: Story = {
	args: {
		nodeId: "01KYWKVPF8BEJ5C5XWN657AS0YW",
		nodeName: "Tokyo edge 01",
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const link = await canvas.findByRole("link", { name: /Tokyo edge 01/i });
		expect(link).toHaveAttribute("href", "/nodes/01KYWKVPF8BEJ5C5XWN657AS0YW");
		expect(link).toHaveAttribute("title", "01KYWKVPF8BEJ5C5XWN657AS0YW");
	},
};

export const LongName: Story = {
	args: {
		nodeId: "01KYWKVPF8BEJ5C5XWN657AS0YW",
		nodeName: "Tokyo edge relay with a deliberately long descriptive name",
	},
};

export const NameUnavailable: Story = {
	args: {
		nodeId: "01KYWKVPF8BEJ5C5XWN657AS0YW",
		nodeName: "   ",
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		expect(canvas.queryByRole("link")).toBeNull();
		expect(
			await canvas.findByText("01KYWKVPF8BEJ5C5XWN657AS0YW"),
		).toBeInTheDocument();
	},
};

export const StateMatrix: Story = {
	args: {
		nodeId: "01KYWKVPF8BEJ5C5XWN657AS0YW",
		nodeName: "Tokyo edge 01",
	},
	render: () => (
		<div className="flex min-h-[720px] flex-col justify-center gap-10">
			<div className="space-y-2">
				<p className="text-sm text-muted-foreground">Resolved</p>
				<NodeNameLink
					nodeId="01KYWKVPF8BEJ5C5XWN657AS0YW"
					nodeName="Tokyo edge 01"
				/>
			</div>
			<div className="space-y-2">
				<p className="text-sm text-muted-foreground">Long name</p>
				<NodeNameLink
					nodeId="01KYWKVPF8BEJ5C5XWN657AS0YW"
					nodeName="Tokyo edge relay with a deliberately long descriptive name"
				/>
			</div>
			<div className="space-y-2">
				<p className="text-sm text-muted-foreground">Name unavailable</p>
				<NodeNameLink nodeId="01KYWKVPF8BEJ5C5XWN657AS0YW" nodeName="   " />
			</div>
		</div>
	),
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		expect(await canvas.findAllByRole("link")).toHaveLength(2);
		expect(
			await canvas.findByText("01KYWKVPF8BEJ5C5XWN657AS0YW"),
		).toBeInTheDocument();
	},
};
