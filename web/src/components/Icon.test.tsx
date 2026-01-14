import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { Icon } from "./Icon";
import { UiPrefsProvider } from "./UiPrefs";

describe("<Icon />", () => {
	it("renders a tabler icon with aria-label", () => {
		render(
			<UiPrefsProvider>
				<Icon name="tabler:server" ariaLabel="Server" />
			</UiPrefsProvider>,
		);

		expect(screen.getByLabelText("Server")).toBeInTheDocument();
	});

	it("rejects non-tabler icons for plan #0010", () => {
		expect(() =>
			render(
				<UiPrefsProvider>
					<Icon name="mdi:home" />
				</UiPrefsProvider>,
			),
		).toThrow(/tabler:/);
	});
});
