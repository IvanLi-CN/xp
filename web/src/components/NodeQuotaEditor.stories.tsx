import type { Meta, StoryObj } from "@storybook/react";
import type { ComponentPropsWithoutRef } from "react";
import { useEffect, useRef, useState } from "react";

import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";

import { formatQuotaBytesHuman } from "../utils/quota";
import { NodeQuotaEditor, type NodeQuotaEditorValue } from "./NodeQuotaEditor";

type NodeQuotaEditorStoryProps = Omit<
	ComponentPropsWithoutRef<typeof NodeQuotaEditor>,
	"onApply"
> & {
	autoOpen?: boolean;
	rejectMessage?: string;
};

function formatStoryValue(value: NodeQuotaEditorValue) {
	return value === "mixed" ? "Mixed" : formatQuotaBytesHuman(value);
}

function NodeQuotaEditorStory({
	value,
	disabled = false,
	autoOpen = false,
	rejectMessage,
}: NodeQuotaEditorStoryProps) {
	const [currentValue, setCurrentValue] = useState<NodeQuotaEditorValue>(value);
	const containerRef = useRef<HTMLDivElement | null>(null);

	useEffect(() => {
		setCurrentValue(value);
	}, [value]);

	useEffect(() => {
		if (!autoOpen) return;
		const frame = requestAnimationFrame(() => {
			containerRef.current
				?.querySelector<HTMLButtonElement>("button[aria-expanded]")
				?.click();
		});
		return () => cancelAnimationFrame(frame);
	}, [autoOpen]);

	return (
		<Card className="w-[380px]">
			<CardHeader>
				<CardTitle className="text-base">Node quota</CardTitle>
				<CardDescription>
					Inline quota editor used in node and quota policy surfaces.
				</CardDescription>
			</CardHeader>
			<CardContent className="space-y-3">
				<div className="rounded-xl border border-border/70 bg-muted/30 p-3 text-sm">
					<div className="text-xs uppercase tracking-[0.12em] text-muted-foreground">
						Current value
					</div>
					<div className="mt-1 font-mono text-foreground">
						{formatStoryValue(currentValue)}
					</div>
				</div>
				<div ref={containerRef}>
					<NodeQuotaEditor
						value={currentValue}
						disabled={disabled}
						onApply={async (nextBytes) => {
							await new Promise((resolve) => window.setTimeout(resolve, 200));
							if (rejectMessage) {
								throw new Error(rejectMessage);
							}
							setCurrentValue(nextBytes);
						}}
					/>
				</div>
			</CardContent>
		</Card>
	);
}

const GiB = 1024 ** 3;

const meta = {
	title: "Components/NodeQuotaEditor",
	component: NodeQuotaEditorStory,
	tags: ["autodocs", "coverage-ui"],
	parameters: {
		layout: "centered",
		docs: {
			description: {
				component:
					"Inline quota editor for node-level overrides. Stories cover the default closed trigger, the mixed-value edge case, a disabled trigger, and the popover editing shell. `onApply` still resolves bytes and surfaces rejected promises as inline errors in the component itself.",
			},
		},
	},
	args: {
		value: 10 * GiB,
		disabled: false,
		autoOpen: false,
	},
} satisfies Meta<typeof NodeQuotaEditorStory>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const MixedValue: Story = {
	args: {
		value: "mixed",
	},
};

export const Disabled: Story = {
	args: {
		disabled: true,
	},
};

export const EditingPopover: Story = {
	args: {
		value: 16 * GiB,
		autoOpen: true,
	},
};
