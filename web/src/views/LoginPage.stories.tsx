import type { Meta, StoryObj } from "@storybook/react";
import { expect, userEvent, within } from "@storybook/test";
import { useEffect } from "react";

import { PrimaryBackendProvider } from "../backend/PrimaryBackendProvider";
import {
	type PrimaryBackendSnapshot,
	getPrimaryBackendSnapshot,
	resetPrimaryBackendTransportForTests,
} from "../backend/primaryBackend";
import { fixtureCatalog } from "../fixture-policy/catalog";
import { LoginPage } from "./LoginPage";

const meta = {
	title: "Pages/LoginPage",
	render: () => <div />,
	parameters: {
		router: {
			initialEntry: "/login",
		},
		viewport: {
			defaultViewport: "loginDesktop1280",
			viewports: {
				loginDesktop1280: {
					name: "Login desktop (1280x900)",
					styles: { height: "900px", width: "1280px" },
					type: "desktop",
				},
				loginMobile393: {
					name: "Login mobile (393x852)",
					styles: { height: "852px", width: "393px" },
					type: "mobile",
				},
			},
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

function LoginPageSurface({
	snapshot,
}: {
	snapshot: PrimaryBackendSnapshot;
}) {
	return (
		<div
			data-visual-evidence-surface
			className={[
				"box-border h-[calc(100vh-2rem)] overflow-hidden bg-background",
				"p-[18px] text-foreground sm:p-8",
			].join(" ")}
		>
			<div data-visual-evidence-target className="h-full overflow-hidden">
				<PrimaryBackendProvider snapshot={snapshot}>
					<LoginPage />
				</PrimaryBackendProvider>
			</div>
		</div>
	);
}

function ZeroCandidatesSurface() {
	useEffect(() => {
		window.localStorage.removeItem("xp-primary-backend-profiles");
		resetPrimaryBackendTransportForTests();
	}, []);
	return (
		<div
			data-visual-evidence-surface
			className={[
				"box-border h-[calc(100vh-2rem)] overflow-hidden bg-background",
				"p-[18px] text-foreground sm:p-8",
			].join(" ")}
		>
			<div data-visual-evidence-target className="h-full overflow-hidden">
				<PrimaryBackendProvider>
					<LoginPage />
				</PrimaryBackendProvider>
			</div>
		</div>
	);
}

const preAuthCandidates = [
	{
		origin: fixtureCatalog.url.primaryApi(),
		nodeId: fixtureCatalog.identifier.nodePrimary(),
		nodeName: fixtureCatalog.identifier.nodeNamePrimary(),
		verifiedAt: Date.now(),
		lastError: null,
	},
	{
		origin: fixtureCatalog.url.secondaryApi(),
		nodeId: fixtureCatalog.identifier.nodeSecondary(),
		nodeName: fixtureCatalog.identifier.nodeNameSecondary(),
		verifiedAt: Date.now(),
		lastError: null,
	},
];

function preAuthSnapshot(
	override: Partial<PrimaryBackendSnapshot> = {},
): PrimaryBackendSnapshot {
	return {
		...getPrimaryBackendSnapshot(),
		clusterId: fixtureCatalog.cluster.fixture53(),
		primaryOrigin: fixtureCatalog.url.secondaryApi(),
		candidates: preAuthCandidates,
		state: "ready",
		...override,
	};
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

export const PreAuthBackendRecovery: Story = {
	render: () => <LoginPageSurface snapshot={preAuthSnapshot()} />,
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
		const trigger = await canvas.findByRole("button", {
			name: "Select service node",
		});
		await expect(trigger).toBeVisible();
		await userEvent.click(trigger);
		const body = within(canvasElement.ownerDocument.body);
		await expect(
			await body.findByRole("menuitem", { name: /fixture-node-a/ }),
		).toBeVisible();
		await expect(
			await body.findByRole("menuitem", { name: /fixture-node-b/ }),
		).toBeVisible();
		await userEvent.keyboard("{Escape}");
		await expect(trigger).toHaveFocus();
	},
};

export const ZeroCandidates: Story = {
	render: () => <ZeroCandidatesSurface />,
	parameters: {
		mockApi: {
			adminToken: null,
		},
		router: {
			initialEntry: "/__story",
		},
		viewport: {
			defaultViewport: "loginMobile393",
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const trigger = canvas.getByRole("button", { name: "Select service node" });
		await expect(trigger).toBeVisible();
		await userEvent.click(trigger);
		const body = within(canvasElement.ownerDocument.body);
		await expect(body.getByRole("menu")).toBeVisible();
		await expect(body.getByRole("menuitem", { name: /Current/ })).toBeVisible();
		await userEvent.keyboard("{Escape}");
		await expect(trigger).toHaveFocus();
	},
};
