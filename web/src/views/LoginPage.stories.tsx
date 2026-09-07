import type { Meta, StoryObj } from "@storybook/react";
import { expect, userEvent, within } from "@storybook/test";
import { useEffect } from "react";

import { LoginPage } from "./LoginPage";

const meta = {
	title: "Pages/LoginPage",
	render: () => <div />,
	parameters: {
		router: {
			initialEntry: "/login",
		},
	},
} satisfies Meta;

export default meta;

type Story = StoryObj<typeof meta>;

function StaticCompatibilityPendingLoginPage() {
	useEffect(() => {
		const previousFetch = window.fetch;
		window.fetch = async (input, init) => {
			const request =
				input instanceof Request ? input : new Request(input, init);
			const url = new URL(request.url, window.location.origin);
			if (url.pathname === "/api/admin/alerts") {
				throw new TypeError("Failed to fetch");
			}
			return previousFetch(input, init);
		};
		return () => {
			window.fetch = previousFetch;
		};
	}, []);

	return <LoginPage staticConsole />;
}

export const MissingToken: Story = {
	parameters: {
		mockApi: {
			adminToken: null,
		},
	},
};

export const WithToken: Story = {
	parameters: {
		mockApi: {
			adminToken: "storybook-admin-token",
		},
	},
};

export const StaticCompatibilityPending: Story = {
	render: () => <StaticCompatibilityPendingLoginPage />,
	parameters: {
		mockApi: {
			adminToken: null,
		},
		router: {
			initialEntry: "/__story",
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.type(
			await canvas.findByLabelText("Token"),
			"storybook-unverified-token",
		);
		await userEvent.click(
			await canvas.findByRole("button", { name: "Save & Continue" }),
		);
		await expect(
			await canvas.findByText("Bootstrap compatibility pending."),
		).toBeInTheDocument();
		await expect(
			canvas.getByText("Token is not saved until verification succeeds."),
		).toBeInTheDocument();
	},
};
