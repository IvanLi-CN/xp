import type { Meta, StoryObj } from "@storybook/react";

import { EditorShortcutHint } from "./EditorShortcutHint";

const meta: Meta<typeof EditorShortcutHint> = {
	title: "Components/EditorShortcutHint",
	component: EditorShortcutHint,
	tags: ["autodocs", "coverage-ui"],
	parameters: {
		docs: {
			description: {
				component:
					"Compact editor shortcut hint used under CodeMirror-based editors. Keys render as separate keycaps and the row stays single-line with horizontal overflow.",
			},
		},
	},
};

export default meta;
type Story = StoryObj<typeof EditorShortcutHint>;

export const Mac: Story = {
	args: {
		platform: "mac",
	},
};

export const Windows: Story = {
	args: {
		platform: "windows",
	},
};

export const Narrow: Story = {
	args: {
		platform: "windows",
	},
	decorators: [
		(StoryComponent) => (
			<div className="w-[360px] rounded-xl border border-border bg-card p-3">
				<StoryComponent />
			</div>
		),
	],
};
