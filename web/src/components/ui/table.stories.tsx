import type { Meta, StoryObj } from "@storybook/react";

import {
	Table,
	TableBody,
	TableCaption,
	TableCell,
	TableFooter,
	TableHead,
	TableHeader,
	TableRow,
} from "./table";

const meta = {
	title: "UI/Table",
	component: Table,
	tags: ["autodocs", "coverage-ui"],
	parameters: {
		layout: "padded",
		docs: {
			description: {
				component:
					"Low-level table primitive behind admin data grids. Stories keep zebra-like row rhythm, footer totals, and a narrow viewport-safe wrapper visible in the docs page.",
			},
		},
	},
} satisfies Meta<typeof Table>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Default: Story = {
	render: () => (
		<Table>
			<TableCaption>Recent node runtime summary.</TableCaption>
			<TableHeader>
				<TableRow>
					<TableHead>Node</TableHead>
					<TableHead>Status</TableHead>
					<TableHead>Updated</TableHead>
				</TableRow>
			</TableHeader>
			<TableBody>
				<TableRow>
					<TableCell>tokyo-1</TableCell>
					<TableCell>up</TableCell>
					<TableCell>2026-03-09 08:00</TableCell>
				</TableRow>
				<TableRow>
					<TableCell>osaka-1</TableCell>
					<TableCell>partial</TableCell>
					<TableCell>2026-03-09 07:58</TableCell>
				</TableRow>
			</TableBody>
			<TableFooter>
				<TableRow>
					<TableCell>Total</TableCell>
					<TableCell colSpan={2}>2 nodes</TableCell>
				</TableRow>
			</TableFooter>
		</Table>
	),
};
