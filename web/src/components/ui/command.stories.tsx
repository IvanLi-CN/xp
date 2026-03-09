import type { Meta, StoryObj } from "@storybook/react";

import {
	Command,
	CommandDialog,
	CommandEmpty,
	CommandGroup,
	CommandInput,
	CommandItem,
	CommandList,
	CommandSeparator,
	CommandShortcut,
} from "./command";

const meta = {
	title: "UI/Command",
	component: Command,
	tags: ["autodocs", "coverage-ui"],
	parameters: {
		layout: "padded",
		docs: {
			description: {
				component:
					"Command palette primitive for quick navigation and placeholder actions. The stories document both the inline shell and the dialog-wrapped variant used by the application shell.",
			},
		},
	},
} satisfies Meta<typeof Command>;

export default meta;

type Story = StoryObj<typeof meta>;

function CommandBody() {
	return (
		<>
			<CommandInput placeholder="Search commands..." />
			<CommandList>
				<CommandEmpty>No results.</CommandEmpty>
				<CommandGroup heading="Navigate">
					<CommandItem>
						Dashboard
						<CommandShortcut>G D</CommandShortcut>
					</CommandItem>
					<CommandItem>
						Nodes
						<CommandShortcut>G N</CommandShortcut>
					</CommandItem>
				</CommandGroup>
				<CommandSeparator />
				<CommandGroup heading="Actions">
					<CommandItem>Create endpoint</CommandItem>
					<CommandItem>Open quota policy</CommandItem>
				</CommandGroup>
			</CommandList>
		</>
	);
}

export const Inline: Story = {
	render: () => (
		<div className="w-[520px] rounded-2xl border border-border/70">
			<Command>
				<CommandBody />
			</Command>
		</div>
	),
};

export const DialogMode: Story = {
	render: () => (
		<CommandDialog open>
			<CommandBody />
		</CommandDialog>
	),
};
