import { render } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AppLayout } from "./AppLayout";

const { mockAppShell } = vi.hoisted(() => ({
	mockAppShell: vi.fn(),
}));

vi.mock("./AppShell", () => ({
	AppShell: (props: unknown) => {
		mockAppShell(props);
		return null;
	},
}));

describe("<AppLayout />", () => {
	beforeEach(() => {
		vi.clearAllMocks();
	});

	it("registers the Tools navigation item under Settings", () => {
		render(<AppLayout />);

		expect(mockAppShell).toHaveBeenCalledTimes(1);
		const props = mockAppShell.mock.calls[0]?.[0] as unknown as {
			navGroups: Array<{
				title: string;
				items: Array<{ label: string; to: string }>;
			}>;
		};
		const settingsGroup = props.navGroups.find(
			(group) => group.title === "Settings",
		);
		expect(settingsGroup?.items).toEqual(
			expect.arrayContaining([
				expect.objectContaining({ label: "Tools", to: "/tools" }),
			]),
		);
	});
});
