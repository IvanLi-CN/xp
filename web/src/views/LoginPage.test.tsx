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
	probeBackendOrigin: vi.fn(),
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
		hydratePrimaryBackendProfile: vi.fn(),
		isStaticWebConsole: mocks.isStaticWebConsole,
		probeBackendOrigin: mocks.probeBackendOrigin,
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
		mocks.probeBackendOrigin.mockReset();
		mocks.probeBackendCandidate.mockReset();
		mocks.selectPreAuthBackend.mockReset();
		mocks.isStaticWebConsole.mockReturnValue(false);
		mocks.probeBackendOrigin.mockResolvedValue({ clusterId: "cluster-84" });
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
		expect(mocks.probeBackendOrigin).toHaveBeenCalledWith({
			origin: window.location.origin,
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

	it("does not verify login_token after a pre-auth mutation timeout", async () => {
		mocks.selectPreAuthBackend.mockResolvedValue({ timedOut: true });

		render(
			<UiPrefsProvider>
				<LoginPage />
			</UiPrefsProvider>,
		);

		await waitFor(() => {
			expect(screen.getByRole("alert")).toHaveTextContent(
				"previous write result is unknown",
			);
		});
		expect(mocks.verifyAdminToken).not.toHaveBeenCalled();
		expect(localStorage.getItem(ADMIN_TOKEN_STORAGE_KEY)).toBeNull();
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
		mocks.selectPreAuthBackend.mockResolvedValue({ timedOut: true });

		render(
			<UiPrefsProvider>
				<LoginPage />
			</UiPrefsProvider>,
		);

		await waitFor(() => {
			expect(mocks.probeBackendCandidate).toHaveBeenCalledWith(
				expect.objectContaining({
					origin: secondary,
					clusterId: "cluster-84",
				}),
			);
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
		expect(screen.getByRole("alert")).toHaveTextContent(
			"previous write result is unknown",
		);
	});

	it("keeps a failed candidate and retries its public probe", async () => {
		window.history.pushState({}, "", "/login");
		const secondary = "https://secondary.example";
		let failed = true;
		mocks.backend.clusterId = "cluster-84";
		mocks.backend.candidates = [
			{
				origin: secondary,
				nodeId: "node-secondary",
				nodeName: "Secondary node",
				verifiedAt: Date.now(),
				lastError: null,
			},
		];
		mocks.probeBackendCandidate.mockImplementation(
			async ({ origin }: { origin: string }) => {
				if (origin === secondary && failed) {
					throw new Error("secondary probe failed");
				}
			},
		);

		render(
			<UiPrefsProvider>
				<LoginPage />
			</UiPrefsProvider>,
		);

		const user = userEvent.setup();
		await user.click(
			screen.getByRole("button", { name: "Select service node" }),
		);
		await waitFor(() => {
			expect(screen.getByRole("alert")).toHaveTextContent(
				"secondary probe failed",
			);
		});
		failed = false;
		await user.click(screen.getByRole("menuitem", { name: /Secondary node/ }));

		await waitFor(() => {
			expect(mocks.probeBackendCandidate).toHaveBeenCalledTimes(2);
			expect(
				screen.getByRole("menuitem", { name: /Secondary node/ }),
			).toHaveTextContent("Ready");
		});
		expect(mocks.backend.candidates).toHaveLength(1);
	});

	it("allows retrying a failed current-page probe", async () => {
		window.history.pushState({}, "", "/login");
		let failed = true;
		mocks.probeBackendOrigin.mockImplementation(async () => {
			if (failed) throw new Error("current page probe failed");
			return { clusterId: "cluster-84" };
		});

		render(
			<UiPrefsProvider>
				<LoginPage />
			</UiPrefsProvider>,
		);

		const user = userEvent.setup();
		await user.click(
			screen.getByRole("button", { name: "Select service node" }),
		);
		await waitFor(() => {
			expect(screen.getByRole("alert")).toHaveTextContent(
				"current page probe failed",
			);
		});
		failed = false;
		await user.click(screen.getByRole("menuitem", { name: /localhost:3000/ }));

		await waitFor(() => {
			expect(mocks.probeBackendOrigin).toHaveBeenCalledTimes(2);
			expect(screen.queryByRole("alert")).not.toBeInTheDocument();
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
		await waitFor(() => {
			expect(
				screen.getByRole("button", { name: "Save & Continue" }),
			).not.toBeDisabled();
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

	it("keeps static login links on the bootstrap transport", async () => {
		window.history.pushState({}, "", "/login?login_token=static-token");
		mocks.isStaticWebConsole.mockReturnValue(true);
		mocks.verifyAdminToken.mockResolvedValue(undefined);

		render(
			<UiPrefsProvider>
				<LoginPage />
			</UiPrefsProvider>,
		);

		await waitFor(() => {
			expect(mocks.verifyAdminToken).toHaveBeenCalledWith("static-token");
		});
		expect(mocks.selectPreAuthBackend).not.toHaveBeenCalled();
		expect(
			screen.queryByRole("button", { name: "Select service node" }),
		).not.toBeInTheDocument();
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
