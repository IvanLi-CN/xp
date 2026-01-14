import { render, screen, waitFor } from "@testing-library/react";
import { useEffect } from "react";
import { describe, expect, it } from "vitest";

import { DataTable } from "./DataTable";
import { UiPrefsProvider, useUiPrefs } from "./UiPrefs";

function SetDensity({ density }: { density: "comfortable" | "compact" }) {
	const prefs = useUiPrefs();
	useEffect(() => {
		prefs.setDensity(density);
	}, [density, prefs]);
	return null;
}

describe("<DataTable />", () => {
	it("renders headers and rows", () => {
		render(
			<UiPrefsProvider>
				<DataTable
					headers={[
						{ key: "id", label: "ID" },
						{ key: "status", label: "Status", align: "right" },
					]}
				>
					<tr>
						<td className="font-mono text-xs">node-1</td>
						<td className="text-right">ok</td>
					</tr>
				</DataTable>
			</UiPrefsProvider>,
		);

		expect(screen.getByText("ID")).toBeInTheDocument();
		expect(screen.getByText("Status")).toBeInTheDocument();
		expect(screen.getByText("node-1")).toBeInTheDocument();
		expect(screen.getByText("ok")).toBeInTheDocument();

		const statusHeader = screen.getByText("Status").closest("th");
		expect(statusHeader).toHaveClass("text-right");
	});

	it("uses compact density from UiPrefs", () => {
		const { container } = render(
			<UiPrefsProvider>
				<SetDensity density="compact" />
				<DataTable headers={[{ key: "id", label: "ID" }]}>
					<tr>
						<td>node-1</td>
					</tr>
				</DataTable>
			</UiPrefsProvider>,
		);

		const table = container.querySelector("table");
		expect(table).not.toBeNull();
		return waitFor(() => {
			expect(table).toHaveClass("table-sm");
		});
	});
});
