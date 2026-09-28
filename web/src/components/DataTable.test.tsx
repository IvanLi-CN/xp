import { render, screen, waitFor } from "@testing-library/react";
import { useEffect } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

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
	afterEach(() => {
		vi.restoreAllMocks();
	});

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
			expect(table).toHaveClass("xp-table-compact");
		});
	});

	it("exposes an accessible local scroll affordance when columns overflow", async () => {
		vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockImplementation(
			function getClientWidth(this: HTMLElement) {
				return this.classList.contains("xp-table-wrap") ? 280 : 0;
			},
		);
		vi.spyOn(HTMLElement.prototype, "scrollWidth", "get").mockImplementation(
			function getScrollWidth(this: HTMLElement) {
				return this.classList.contains("xp-table-wrap") ? 360 : 0;
			},
		);

		render(
			<UiPrefsProvider>
				<DataTable
					ariaLabel="Endpoint inventory"
					headers={[{ key: "id", label: "ID" }]}
				>
					<tr>
						<td>endpoint-1</td>
					</tr>
				</DataTable>
			</UiPrefsProvider>,
		);

		const region = await screen.findByRole("region", {
			name: "Endpoint inventory",
		});
		expect(region).toHaveAttribute("data-overflowing", "true");
		expect(screen.getByText("More columns")).toBeInTheDocument();
		expect(region).toHaveAttribute("tabindex", "0");
	});

	it("remeasures when table content changes size", async () => {
		let tableWidth = 280;
		vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockImplementation(
			function getClientWidth(this: HTMLElement) {
				return this.classList.contains("xp-table-wrap") ? 280 : 0;
			},
		);
		vi.spyOn(HTMLElement.prototype, "scrollWidth", "get").mockImplementation(
			function getScrollWidth(this: HTMLElement) {
				return this.classList.contains("xp-table-wrap") ? tableWidth : 0;
			},
		);

		const { rerender } = render(
			<UiPrefsProvider>
				<DataTable headers={[{ key: "id", label: "ID" }]}>
					<tr>
						<td>endpoint-1</td>
					</tr>
				</DataTable>
			</UiPrefsProvider>,
		);

		await waitFor(() => {
			expect(screen.queryByText("More columns")).not.toBeInTheDocument();
		});
		tableWidth = 360;
		rerender(
			<UiPrefsProvider>
				<DataTable headers={[{ key: "id", label: "ID" }]}>
					<tr>
						<td>endpoint-1-expanded</td>
					</tr>
				</DataTable>
			</UiPrefsProvider>,
		);

		await waitFor(() => {
			expect(screen.getByText("More columns")).toBeInTheDocument();
		});
	});
});
