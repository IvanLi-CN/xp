import type { Meta, StoryObj } from "@storybook/react";

import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";

import { AuthGate } from "./AuthGate";

const meta = {
	title: "Components/AuthGate",
	component: AuthGate,
	tags: ["autodocs", "coverage-ui"],
	args: {
		children: <div>Protected content</div>,
		fallback: undefined,
	},
	parameters: {
		docs: {
			description: {
				component:
					"Route guard that checks whether an admin token is available before rendering protected content. The authenticated story uses the shared card primitives and documents the protected-state shell.",
			},
		},
	},
} satisfies Meta<typeof AuthGate>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Unauthenticated: Story = {
	parameters: { mockApi: { adminToken: null } },
	render: () => (
		<AuthGate>
			<div>Should not render when unauthenticated.</div>
		</AuthGate>
	),
};

export const Authenticated: Story = {
	parameters: { mockApi: { adminToken: "storybook-token" } },
	render: () => (
		<AuthGate>
			<Card className="max-w-md">
				<CardHeader>
					<CardTitle>Authenticated</CardTitle>
					<CardDescription>Token detected in localStorage.</CardDescription>
				</CardHeader>
				<CardContent className="pt-0 text-sm text-muted-foreground">
					Protected content renders normally.
				</CardContent>
			</Card>
		</AuthGate>
	),
};
