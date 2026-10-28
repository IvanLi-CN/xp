import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { BackendApiError } from "../api/backendError";
import { UiPrefsProvider } from "../components/UiPrefs";
import { ADMIN_TOKEN_STORAGE_KEY } from "../components/auth";
import { LoginPage } from "./LoginPage";

const mocks = vi.hoisted(() => ({
	navigate: vi.fn(),
	verifyAdminToken: vi.fn(),
	isStaticWebConsole: vi.fn(),
	probeBackendCandidate: vi.fn(),
	selectPreAuthBackend: vi.fn(),
	backend: {
		clusterId: null as string | null,
		primaryOrigin: "",
		candidates: [] as Array<{
			origin: string;
			nodeId: string;
			nodeName: string;
			verifiedAt: number | null;
			lastError: string | null;
		}>,
		generation: 0,
		state: "ready" as const,
		pendingMutations: 0,
		unknownMutationCount: 0,
		lastSwitchTimedOut: false,
	},
}));

vi.mock("@tanstack/react-router", async () => {
	const actual = await vi.importActual<object>("@tanstack/react-router");
	return {
		...actual,
		useNavigate: () => mocks.navigate,
	};
});

vi.mock("../api/adminAuth", () => ({
	verifyAdminToken: mocks.verifyAdminToken,
}));

vi.mock("../backend/primaryBackend", async () => {
	const actual = await vi.importActual<object>("../backend/primaryBackend");
	return {
		...actual,
		isStaticWebConsole: mocks.isStaticWebConsole,
		probeBackendCandidate: mocks.probeBackendCandidate,
		selectPreAuthBackend: mocks.selectPreAuthBackend,
	};
});

vi.mock("../backend/PrimaryBackendProvider", () => ({
	usePrimaryBackend: () => mocks.backend,
}));

describe("<LoginPage />", () => {
	beforeEach(() => {
		const store = new Map<string, string>();
		Object.defineProperty(globalThis, "localStorage", {
			value: {
				getItem: (key: string) => store.get(key) ?? null,
				setItem: (key: string, value: string) => {
					store.set(key, value);
				},
				removeItem: (key: string) => {
					store.delete(key);
				},
			},
			configurable: true,
		});

		try {
			localStorage.removeItem(ADMIN_TOKEN_STORAGE_KEY);
		} catch {
			// ignore
		}
		mocks.navigate.mockReset();
		mocks.verifyAdminToken.mockReset();
		mocks.isStaticWebConsole.mockReset();
		mocks.probeBackendCandidate.mockReset();
		mocks.selectPreAuthBackend.mockReset();
		mocks.isStaticWebConsole.mockReturnValue(false);
		mocks.probeBackendCandidate.mockResolvedValue(undefined);
		mocks.selectPreAuthBackend.mockResolvedValue({ timedOut: false });
		mocks.backend.clusterId = null;
		mocks.backend.primaryOrigin = window.location.origin;
		mocks.backend.candidates = [];
		window.history.pushState(
			{},
			"",
			"/login?login_token=test.jwt.token&redirect=/nodes%3Fview%3Dtable%23history",
		);
	});

	it("consumes login_token from URL, verifies, stores, removes it from the address bar, and navigates back to redirect", async () => {
		mocks.verifyAdminToken.mockResolvedValue(undefined);

		render(
			<UiPrefsProvider>
				<LoginPage />
			</UiPrefsProvider>,
		);

		await waitFor(() => {
			expect(mocks.verifyAdminToken).toHaveBeenCalledWith("test.jwt.token");
		});

		await waitFor(() => {
			expect(localStorage.getItem(ADMIN_TOKEN_STORAGE_KEY)).toBe(
				"test.jwt.token",
			);
		});

		expect(window.location.search).toBe(
			"?redirect=%2Fnodes%3Fview%3Dtable%23history",
		);
		expect(mocks.navigate).toHaveBeenCalledWith({
			href: "/nodes?view=table#history",
		});
	});

	it("selects a probed profile candidate before token verification", async () => {
		window.history.pushState({}, "", "/login");
		const secondary = "https://secondary.example";
		mocks.backend.clusterId = "cluster-84";
		mocks.backend.candidates = [
			{
				origin: secondary,
				nodeId: "node-secondary",
				nodeName: "",
				verifiedAt: Date.now(),
				lastError: null,
			},
		];

		render(
			<UiPrefsProvider>
				<LoginPage />
			</UiPrefsProvider>,
		);

		await waitFor(() => {
			expect(mocks.probeBackendCandidate).toHaveBeenCalledWith({
				origin: secondary,
				clusterId: "cluster-84",
			});
		});
		const user = userEvent.setup();
		await user.click(
			screen.getByRole("button", { name: "Select service node" }),
		);
		await user.click(
			await screen.findByRole("menuitem", { name: /secondary\.example/ }),
		);

		await waitFor(() => {
			expect(mocks.selectPreAuthBackend).toHaveBeenCalledWith(secondary);
		});
	});

	it("keeps a selected candidate and unsaved token after a 401", async () => {
		window.history.pushState({}, "", "/login");
		const secondary = "https://secondary.example";
		mocks.backend.clusterId = "cluster-84";
		mocks.backend.primaryOrigin = secondary;
		mocks.backend.candidates = [
			{
				origin: secondary,
				nodeId: "node-secondary",
				nodeName: "Secondary node",
				verifiedAt: Date.now(),
				lastError: null,
			},
		];
		mocks.verifyAdminToken.mockRejectedValue(
			new BackendApiError({
				status: 401,
				message: "missing or invalid authorization token",
			}),
		);

		render(
			<UiPrefsProvider>
				<LoginPage />
			</UiPrefsProvider>,
		);
		fireEvent.change(screen.getByLabelText("Token"), {
			target: { value: "unverified-token" },
		});
		fireEvent.click(screen.getByRole("button", { name: "Save & Continue" }));

		await waitFor(() => {
			expect(
				screen.getByText("401: missing or invalid authorization token"),
			).toBeInTheDocument();
		});
		expect(screen.getByLabelText("Token")).toHaveValue("unverified-token");
		expect(localStorage.getItem(ADMIN_TOKEN_STORAGE_KEY)).toBeNull();
		expect(
			screen.getByRole("button", { name: "Select service node" }),
		).toBeInTheDocument();
	});

	it("shows compatibility pending without storing an unverified static-console token", async () => {
		window.history.pushState({}, "", "/login");
		mocks.isStaticWebConsole.mockReturnValue(true);
		mocks.verifyAdminToken.mockRejectedValue(new TypeError("Failed to fetch"));

		render(
			<UiPrefsProvider>
				<LoginPage />
			</UiPrefsProvider>,
		);

		fireEvent.change(screen.getByLabelText("Token"), {
			target: { value: "unverified-token" },
		});
		fireEvent.click(screen.getByRole("button", { name: "Save & Continue" }));

		await waitFor(() => {
			expect(
				screen.getByText("Bootstrap compatibility pending."),
			).toBeInTheDocument();
		});

		expect(
			screen.getByText("Token is not saved until verification succeeds."),
		).toBeInTheDocument();
		expect(screen.queryByText("No token set.")).not.toBeInTheDocument();
		expect(screen.queryByText("Failed to fetch")).not.toBeInTheDocument();
		expect(
			screen.queryByRole("button", { name: "Select service node" }),
		).not.toBeInTheDocument();
		expect(localStorage.getItem(ADMIN_TOKEN_STORAGE_KEY)).toBeNull();
		expect(screen.getByLabelText("Token")).toHaveValue("unverified-token");
	});

	it("falls back to root for invalid redirect targets", async () => {
		mocks.verifyAdminToken.mockResolvedValue(undefined);
		window.history.pushState(
			{},
			"",
			"/login?login_token=test.jwt.token&redirect=https%3A%2F%2Fevil.example.com",
		);

		render(
			<UiPrefsProvider>
				<LoginPage />
			</UiPrefsProvider>,
		);

		await waitFor(() => {
			expect(mocks.navigate).toHaveBeenCalledWith({ href: "/" });
		});
	});
});
