import type { Meta, StoryObj } from "@storybook/react";

import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";

import { AuthGate } from "./AuthGate";
import { clearAdminToken, writeAdminToken } from "./auth";

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
	render: () => {
		clearAdminToken();
		return (
			<AuthGate>
				<div>Should not render when unauthenticated.</div>
			</AuthGate>
		);
	},
};

export const Authenticated: Story = {
	render: () => {
		writeAdminToken("storybook-token");
		return (
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
		);
	},
};
