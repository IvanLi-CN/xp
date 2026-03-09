import type { Meta, StoryObj } from "@storybook/react";

import { Tabs, TabsContent, TabsList, TabsTrigger } from "./tabs";

const meta = {
	title: "UI/Tabs",
	component: Tabs,
	tags: ["autodocs", "coverage-ui"],
	parameters: {
		layout: "centered",
		docs: {
			description: {
				component:
					"Tabs primitive for section switching inside detail pages and editors. Stories cover a standard content switcher and an overflow-heavy list to make density changes easy to inspect.",
			},
		},
	},
} satisfies Meta<typeof Tabs>;

export default meta;

type Story = StoryObj<typeof meta>;

export const ContentSwitching: Story = {
	render: () => (
		<Tabs defaultValue="runtime" className="w-[420px] space-y-4">
			<TabsList className="w-full justify-start">
				<TabsTrigger value="runtime">Runtime</TabsTrigger>
				<TabsTrigger value="metadata">Metadata</TabsTrigger>
				<TabsTrigger value="quota">Quota</TabsTrigger>
			</TabsList>
			<TabsContent
				value="runtime"
				className="rounded-2xl border border-border/70 p-4 text-sm"
			>
				Service runtime summary.
			</TabsContent>
			<TabsContent
				value="metadata"
				className="rounded-2xl border border-border/70 p-4 text-sm"
			>
				Node metadata form shell.
			</TabsContent>
			<TabsContent
				value="quota"
				className="rounded-2xl border border-border/70 p-4 text-sm"
			>
				Quota reset editor.
			</TabsContent>
		</Tabs>
	),
};

export const OverflowList: Story = {
	render: () => (
		<Tabs defaultValue="one" className="w-[520px] space-y-4">
			<div className="overflow-x-auto">
				<TabsList className="min-w-max justify-start">
					{["one", "two", "three", "four", "five", "six"].map((tab) => (
						<TabsTrigger key={tab} value={tab} className="capitalize">
							{tab}
						</TabsTrigger>
					))}
				</TabsList>
			</div>
			<TabsContent
				value="one"
				className="rounded-2xl border border-border/70 p-4 text-sm"
			>
				Overflow-friendly tab strip.
			</TabsContent>
		</Tabs>
	),
};
