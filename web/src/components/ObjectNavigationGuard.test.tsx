import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import {
	type ObjectNavigationDirtySection,
	ObjectNavigationGuardProvider,
	useObjectNavigationDirtySections,
	useObjectNavigationGuard,
} from "./ObjectNavigationGuard";

function GuardHarness({
	sections,
	onNavigate,
}: {
	sections: ObjectNavigationDirtySection[];
	onNavigate: () => void;
}) {
	const { requestNavigation } = useObjectNavigationGuard();
	useObjectNavigationDirtySections("object", sections);

	return (
		<button type="button" onClick={() => requestNavigation(onNavigate)}>
			Open next object
		</button>
	);
}

function renderGuard(
	sections: ObjectNavigationDirtySection[],
	onNavigate = vi.fn(),
) {
	render(
		<ObjectNavigationGuardProvider>
			<GuardHarness sections={sections} onNavigate={onNavigate} />
		</ObjectNavigationGuardProvider>,
	);
	return onNavigate;
}

describe("<ObjectNavigationGuardProvider />", () => {
	it("resolves dirty sections in registration order before navigating", async () => {
		const saveProfile = vi.fn(async () => true);
		const discardAccess = vi.fn();
		const onNavigate = renderGuard([
			{
				id: "profile",
				label: "profile",
				isDirty: () => true,
				save: saveProfile,
				discard: vi.fn(),
			},
			{
				id: "access",
				label: "access",
				isDirty: () => true,
				save: vi.fn(async () => true),
				discard: discardAccess,
			},
		]);

		fireEvent.click(screen.getByRole("button", { name: "Open next object" }));
		expect(
			screen.getByRole("heading", { name: "Unsaved profile changes" }),
		).toBeInTheDocument();

		fireEvent.click(screen.getByRole("button", { name: "Save and continue" }));
		await waitFor(() =>
			expect(
				screen.getByRole("heading", { name: "Unsaved access changes" }),
			).toBeInTheDocument(),
		);
		expect(onNavigate).not.toHaveBeenCalled();

		fireEvent.click(
			screen.getByRole("button", { name: "Discard and continue" }),
		);
		await waitFor(() => expect(onNavigate).toHaveBeenCalledTimes(1));
		expect(saveProfile).toHaveBeenCalledTimes(1);
		expect(discardAccess).toHaveBeenCalledTimes(1);
	});

	it("keeps the current object open when saving a dirty section fails", async () => {
		const onNavigate = renderGuard([
			{
				id: "profile",
				label: "profile",
				isDirty: () => true,
				save: vi.fn(async () => false),
				discard: vi.fn(),
			},
		]);

		fireEvent.click(screen.getByRole("button", { name: "Open next object" }));
		fireEvent.click(screen.getByRole("button", { name: "Save and continue" }));

		await waitFor(() =>
			expect(
				screen.getByRole("heading", { name: "Unsaved profile changes" }),
			).toBeInTheDocument(),
		);
		expect(onNavigate).not.toHaveBeenCalled();
	});
});
