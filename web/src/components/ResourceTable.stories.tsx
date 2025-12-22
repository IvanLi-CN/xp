import type { Meta, StoryObj } from "@storybook/react";

import { ResourceTable } from "./ResourceTable";

const meta: Meta<typeof ResourceTable> = {
	title: "Components/ResourceTable",
	component: ResourceTable,
	args: {
		headers: [
			{ key: "name", label: "name" },
			{ key: "id", label: "id" },
		],
		children: (
			<>
				<tr>
					<td>node-1</td>
					<td className="font-mono">01J...</td>
				</tr>
				<tr>
					<td>node-2</td>
					<td className="font-mono">01K...</td>
				</tr>
			</>
		),
	},
};

export default meta;

type Story = StoryObj<typeof ResourceTable>;

export const Default: Story = {};
