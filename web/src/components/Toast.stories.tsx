import type { Meta, StoryObj } from "@storybook/react";

import { Button } from "./Button";
import { useToast } from "./Toast";

type ToastStoryPanelProps = {
	longMessage?: boolean;
};

function ToastStoryPanel({ longMessage = false }: ToastStoryPanelProps) {
	const { pushToast } = useToast();

	return (
		<div className="flex flex-wrap gap-2">
			<Button
				variant="secondary"
				onClick={() =>
					pushToast({
						variant: "success",
						message: longMessage
							? "Endpoint saved and probe schedule refreshed for the selected node."
							: "Saved.",
					})
				}
			>
				Success
			</Button>
			<Button
				variant="secondary"
				onClick={() =>
					pushToast({
						variant: "error",
						message: longMessage
							? "Quota update failed because the backend rejected the new limit."
							: "Failed to save.",
					})
				}
			>
				Error
			</Button>
			<Button
				variant="secondary"
				onClick={() =>
					pushToast({
						variant: "info",
						message: longMessage
							? "Background sync is still running; results will appear in the table once the stream catches up."
							: "Reconnecting...",
					})
				}
			>
				Info
			</Button>
		</div>
	);
}

const meta = {
	title: "Components/Toast",
	component: ToastStoryPanel,
	tags: ["autodocs", "coverage-ui"],
	parameters: {
		layout: "centered",
		docs: {
			description: {
				component:
					"App-level toast API backed by Sonner. Storybook preview already mounts `ToastProvider`, so these stories exercise the real `useToast()` integration without creating a second toaster instance.",
			},
		},
	},
} satisfies Meta<typeof ToastStoryPanel>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const LongMessage: Story = {
	args: {
		longMessage: true,
	},
};
