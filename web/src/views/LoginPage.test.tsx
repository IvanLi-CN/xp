import { render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { UiPrefsProvider } from "../components/UiPrefs";
import { ADMIN_TOKEN_STORAGE_KEY } from "../components/auth";
import { LoginPage } from "./LoginPage";

const mocks = vi.hoisted(() => ({
	navigate: vi.fn(),
	verifyAdminToken: vi.fn(),
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
		window.history.pushState({}, "", "/login?login_token=test.jwt.token");
	});

	it("consumes login_token from URL, verifies, stores, and removes it from the address bar", async () => {
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

		expect(window.location.search).toBe("");
		expect(mocks.navigate).toHaveBeenCalledWith({ to: "/" });
	});
});
