import type { Meta, StoryObj } from "@storybook/react";
import { expect, userEvent, within } from "@storybook/test";
import { useState } from "react";

import {
	AutocompleteInput,
	type AutocompleteSuggestion,
} from "./AutocompleteInput";

type AutocompleteStoryProps = {
	placeholder?: string;
	suggestionLabel?: string;
	suggestions?: AutocompleteSuggestion[];
};

function AutocompleteInputStory(args: AutocompleteStoryProps) {
	const [value, setValue] = useState("");
	return (
		<div className="w-full max-w-xl p-6">
			<AutocompleteInput
				{...args}
				aria-label="dest"
				placeholder={args.placeholder ?? "oneclient.sfx.ms:443"}
				suggestionLabel={args.suggestionLabel ?? "Open suggestions"}
				suggestions={args.suggestions ?? []}
				value={value}
				onChange={(event) => setValue(event.target.value)}
				onSuggestionSelect={setValue}
			/>
		</div>
	);
}

const meta = {
	title: "Components/AutocompleteInput",
	component: AutocompleteInput,
	tags: ["autodocs"],
	args: {
		placeholder: "oneclient.sfx.ms:443",
		suggestionLabel: "Open suggestions",
		suggestions: [
			{
				value: "hinet-api.example.com:62416",
				label: "hinet-api.example.com:62416",
			},
		],
	},
	render: (args) => <AutocompleteInputStory {...args} />,
} satisfies Meta<typeof AutocompleteInput>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Default: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(await canvas.findByLabelText("dest"));
		await userEvent.click(
			await within(document.body).findByText("hinet-api.example.com:62416"),
		);
		await expect(await canvas.findByLabelText("dest")).toHaveValue(
			"hinet-api.example.com:62416",
		);
	},
};

export const NoSuggestions: Story = {
	args: {
		suggestions: [],
	},
};
