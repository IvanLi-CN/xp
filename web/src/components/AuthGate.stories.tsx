import type { Meta, StoryObj } from "@storybook/react";

import { AuthGate } from "./AuthGate";
import { clearAdminToken, writeAdminToken } from "./auth";

const meta: Meta<typeof AuthGate> = {
	title: "Components/AuthGate",
	component: AuthGate,
};

export default meta;

type Story = StoryObj<typeof AuthGate>;

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
				<div className="card bg-base-100 shadow">
					<div className="card-body">
						<h2 className="card-title">Authenticated</h2>
						<p className="text-sm opacity-70">
							Token detected in localStorage.
						</p>
					</div>
				</div>
			</AuthGate>
		);
	},
};
