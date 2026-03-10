import type { Meta, StoryObj } from "@storybook/react";

import { Badge } from "@/components/ui/badge";
import { TableCell, TableRow } from "@/components/ui/table";

import { DataTable } from "./DataTable";

const headers = [
	{ key: "id", label: "ID" },
	{ key: "name", label: "Name" },
	{ key: "status", label: "Status", align: "right" as const },
];

const meta = {
	title: "Components/DataTable",
	component: DataTable,
	tags: ["autodocs", "coverage-ui"],
	args: {
		headers,
		children: null,
		density: "comfortable",
		caption: <span>2 items</span>,
		tableClassName: undefined,
	},
	parameters: {
		docs: {
			description: {
				component:
					"Shared data table wrapper built on the shadcn table primitives. Use the `density` prop to verify comfortable vs compact spacing while verifying density differences with the shared table surface.",
			},
		},
	},
} satisfies Meta<typeof DataTable>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Comfortable: Story = {
	render: () => (
		<DataTable
			headers={headers}
			density="comfortable"
			caption={<span>2 items</span>}
		>
			<TableRow>
				<TableCell className="font-mono text-xs">node-1</TableCell>
				<TableCell>alpha</TableCell>
				<TableCell className="text-right">
					<Badge variant="success" size="sm">
						ready
					</Badge>
				</TableCell>
			</TableRow>
			<TableRow>
				<TableCell className="font-mono text-xs">node-2</TableCell>
				<TableCell>beta</TableCell>
				<TableCell className="text-right">
					<Badge variant="warning" size="sm">
						degraded
					</Badge>
				</TableCell>
			</TableRow>
		</DataTable>
	),
};

export const Compact: Story = {
	args: {
		density: "compact",
		caption: undefined,
	},
	render: () => (
		<DataTable headers={headers} density="compact">
			<TableRow>
				<TableCell className="font-mono text-xs">node-1</TableCell>
				<TableCell>alpha</TableCell>
				<TableCell className="text-right">
					<Badge variant="success" size="sm">
						ready
					</Badge>
				</TableCell>
			</TableRow>
		</DataTable>
	),
};
