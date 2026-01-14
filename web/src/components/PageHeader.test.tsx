import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { Button } from "./Button";
import { PageHeader } from "./PageHeader";
import { UiPrefsProvider } from "./UiPrefs";

describe("<PageHeader />", () => {
	it("renders title, description, and actions", () => {
		render(
			<UiPrefsProvider>
				<PageHeader
					title="Nodes"
					description="Inspect cluster nodes."
					actions={<Button>Refresh</Button>}
				/>
			</UiPrefsProvider>,
		);

		expect(screen.getByRole("heading", { name: "Nodes" })).toBeInTheDocument();
		expect(screen.getByText("Inspect cluster nodes.")).toBeInTheDocument();
		expect(screen.getByRole("button", { name: "Refresh" })).toBeInTheDocument();
	});
});
