import type { Meta, StoryObj } from "@storybook/react";

import { Icon } from "./Icon";

const meta: Meta<typeof Icon> = {
	title: "Components/Icon",
	component: Icon,
	args: {
		name: "tabler:layout-dashboard",
	},
};

export default meta;

type Story = StoryObj<typeof Icon>;

export const Default: Story = {};

export const Catalog: Story = {
	render: () => (
		<div className="flex flex-wrap items-center gap-4">
			<div className="flex items-center gap-2">
				<Icon name="tabler:layout-dashboard" />
				<span className="font-mono text-sm">tabler:layout-dashboard</span>
			</div>
			<div className="flex items-center gap-2">
				<Icon name="tabler:server" />
				<span className="font-mono text-sm">tabler:server</span>
			</div>
			<div className="flex items-center gap-2">
				<Icon name="tabler:plug" />
				<span className="font-mono text-sm">tabler:plug</span>
			</div>
			<div className="flex items-center gap-2">
				<Icon name="tabler:users" />
				<span className="font-mono text-sm">tabler:users</span>
			</div>
			<div className="flex items-center gap-2">
				<Icon name="tabler:key" />
				<span className="font-mono text-sm">tabler:key</span>
			</div>
		</div>
	),
};
