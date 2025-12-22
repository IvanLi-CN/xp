import type { Meta, StoryObj } from "@storybook/react";

import { Button } from "./Button";
import { ToastProvider, useToast } from "./Toast";

function ToastDemo() {
	const { pushToast } = useToast();

	return (
		<div className="flex gap-2">
			<Button
				variant="secondary"
				onClick={() => pushToast({ variant: "success", message: "Saved." })}
			>
				Success
			</Button>
			<Button
				variant="secondary"
				onClick={() =>
					pushToast({ variant: "error", message: "Failed to save." })
				}
			>
				Error
			</Button>
			<Button
				variant="secondary"
				onClick={() =>
					pushToast({ variant: "info", message: "Reconnecting..." })
				}
			>
				Info
			</Button>
		</div>
	);
}

const meta: Meta<typeof ToastDemo> = {
	title: "Components/Toast",
	component: ToastDemo,
	render: () => (
		<ToastProvider>
			<ToastDemo />
		</ToastProvider>
	),
};

export default meta;

type Story = StoryObj<typeof ToastDemo>;

export const Default: Story = {};
