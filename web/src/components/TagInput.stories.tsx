import type { Meta, StoryObj } from "@storybook/react";
import { useState } from "react";

import { validateRealityServerName } from "../utils/realityServerName";
import { TagInput } from "./TagInput";

type TagInputDemoProps = {
	label: string;
	placeholder?: string;
	helperText?: string;
	disabled?: boolean;
	initialValue: string[];
};

function TagInputDemo({
	label,
	placeholder,
	helperText,
	disabled = false,
	initialValue,
}: TagInputDemoProps) {
	const [tags, setTags] = useState<string[]>(initialValue);

	return (
		<div className="max-w-2xl">
			<TagInput
				label={label}
				value={tags}
				onChange={setTags}
				placeholder={placeholder}
				helperText={helperText}
				disabled={disabled}
				validateTag={validateRealityServerName}
			/>
		</div>
	);
}

const meta: Meta<typeof TagInputDemo> = {
	title: "Components/TagInput",
	component: TagInputDemo,
	args: {
		label: "serverNames",
		placeholder: "oneclient.sfx.ms",
		helperText:
			"Enter a domain and press Enter/Comma to add. First tag is primary. Paste multiple domains to batch add.",
		disabled: false,
	},
};

export default meta;

type Story = StoryObj<typeof TagInputDemo>;

export const Empty: Story = {
	args: {
		initialValue: [],
	},
};

export const Prefilled: Story = {
	args: {
		initialValue: ["oneclient.sfx.ms", "public.sn.files.1drv.com"],
	},
};
