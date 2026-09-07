import type { Meta, StoryObj } from "@storybook/react";

import { LoginBootstrapCompatibilityPending } from "./LoginBootstrapCompatibilityPending";

const meta = {
	title: "Views/LoginBootstrapCompatibilityPending",
	component: LoginBootstrapCompatibilityPending,
	tags: ["autodocs", "coverage-ui"],
	decorators: [
		(Story) => (
			<div
				className="min-h-screen bg-background p-6"
				data-visual-evidence-surface="login-bootstrap-compatibility-pending"
			>
				<div
					className="mx-auto max-w-xl"
					data-visual-evidence-target="login-bootstrap-compatibility-pending"
				>
					<Story />
				</div>
			</div>
		),
	],
} satisfies Meta<typeof LoginBootstrapCompatibilityPending>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Default: Story = {};
