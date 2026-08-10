import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { fetchSubscription } from "../api/subscription";
import { useUserRouteTransientState } from "./useUserRouteTransientState";

vi.mock("../api/subscription", async (importOriginal) => {
	const actual = await importOriginal<typeof import("../api/subscription")>();
	return { ...actual, fetchSubscription: vi.fn() };
});

function TransientStateHarness({ userId }: { userId: string }) {
	const state = useUserRouteTransientState(userId);
	return (
		<>
			<button
				type="button"
				onClick={() =>
					void state.loadSubscriptionPreview("token", "raw", false, (error) =>
						error instanceof Error ? error.message : String(error),
					)
				}
			>
				Load preview
			</button>
			<button type="button" onClick={() => state.setDeleteOpen(true)}>
				Open delete
			</button>
			{state.deleteOpen && state.isCurrentTransientState ? (
				<div>Delete confirmation</div>
			) : null}
			{state.subText ? <div>{state.subText}</div> : null}
		</>
	);
}

describe("useUserRouteTransientState", () => {
	beforeEach(() => {
		vi.resetAllMocks();
	});

	it("rejects an in-flight preview result after a user route change", async () => {
		let resolvePreview = (_value: string) => {};
		vi.mocked(fetchSubscription).mockImplementation(
			() =>
				new Promise<string>((resolve) => {
					resolvePreview = resolve;
				}),
		);
		const view = render(<TransientStateHarness userId="user-a" />);

		fireEvent.click(screen.getByRole("button", { name: "Load preview" }));
		view.rerender(<TransientStateHarness userId="user-b" />);
		resolvePreview("preview-for-user-a");

		await waitFor(() => {
			expect(screen.queryByText("preview-for-user-a")).toBeNull();
		});
	});

	it("closes a user confirmation when the route changes", () => {
		const view = render(<TransientStateHarness userId="user-a" />);
		fireEvent.click(screen.getByRole("button", { name: "Open delete" }));
		expect(screen.getByText("Delete confirmation")).toBeInTheDocument();

		view.rerender(<TransientStateHarness userId="user-b" />);
		expect(screen.queryByText("Delete confirmation")).toBeNull();
	});
});
