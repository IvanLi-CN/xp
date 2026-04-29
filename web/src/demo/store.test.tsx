import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import {
	DemoProvider,
	clearDemoFallbackState,
	hasDemoSession,
	useDemo,
} from "./store";

const originalLocalStorageDescriptor = Object.getOwnPropertyDescriptor(
	globalThis,
	"localStorage",
);

function DemoHarness() {
	const { login, logout, state } = useDemo();

	return (
		<div>
			<p data-testid="role">{state.session?.role ?? "none"}</p>
			<button
				type="button"
				onClick={() =>
					login({
						role: "admin",
						operatorName: "Storage Blocked",
						scenarioId: "normal",
					})
				}
			>
				Login
			</button>
			<button type="button" onClick={logout}>
				Logout
			</button>
		</div>
	);
}

describe("demo store", () => {
	beforeEach(() => {
		clearDemoFallbackState();
		Object.defineProperty(globalThis, "localStorage", {
			value: {
				getItem: () => null,
				setItem: () => {
					throw new Error("storage blocked");
				},
				removeItem: () => {},
			},
			configurable: true,
		});
	});

	afterEach(() => {
		clearDemoFallbackState();
		if (originalLocalStorageDescriptor) {
			Object.defineProperty(
				globalThis,
				"localStorage",
				originalLocalStorageDescriptor,
			);
		}
	});

	it("keeps the route guard session in memory when localStorage writes fail", () => {
		render(
			<DemoProvider>
				<DemoHarness />
			</DemoProvider>,
		);

		expect(hasDemoSession()).toBe(false);

		fireEvent.click(screen.getByRole("button", { name: "Login" }));

		expect(screen.getByTestId("role")).toHaveTextContent("admin");
		expect(hasDemoSession()).toBe(true);

		fireEvent.click(screen.getByRole("button", { name: "Logout" }));

		expect(screen.getByTestId("role")).toHaveTextContent("none");
		expect(hasDemoSession()).toBe(false);
	});

	it("rejects stored demo state without a valid session", () => {
		Object.defineProperty(globalThis, "localStorage", {
			value: {
				getItem: () => JSON.stringify({ nodes: [], users: [] }),
				setItem: () => {},
				removeItem: () => {},
			},
			configurable: true,
		});

		expect(hasDemoSession()).toBe(false);
	});
});
