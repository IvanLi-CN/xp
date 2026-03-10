import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { AccessMatrix, type AccessMatrixCellState } from "./AccessMatrix";

afterEach(() => {
	cleanup();
});

function renderMatrix(args?: {
	cells?: Record<string, Record<string, AccessMatrixCellState>>;
	onToggleCell?: (nodeId: string, protocolId: string) => void;
	onToggleCellEndpoint?: (
		nodeId: string,
		protocolId: string,
		endpointId: string,
		checked: boolean,
	) => void;
}) {
	return render(
		<AccessMatrix
			nodes={[{ nodeId: "node-a", label: "Node A" }]}
			protocols={[{ protocolId: "vless", label: "VLESS" }]}
			cells={
				args?.cells ?? {
					"node-a": {
						vless: {
							value: "off",
							meta: { port: 443, endpointId: "ep-vless" },
						},
					},
				}
			}
			onToggleCell={args?.onToggleCell}
			onToggleCellEndpoint={args?.onToggleCellEndpoint}
		/>,
	);
}

describe("<AccessMatrix />", () => {
	it("toggles a single cell when its text is clicked", () => {
		const onToggleCell = vi.fn();
		renderMatrix({ onToggleCell });

		fireEvent.click(screen.getByText("port 443"));

		expect(onToggleCell).toHaveBeenCalledTimes(1);
		expect(onToggleCell).toHaveBeenCalledWith("node-a", "vless");
	});

	it("does not double toggle when the single-cell checkbox is clicked", () => {
		const onToggleCell = vi.fn();
		renderMatrix({ onToggleCell });

		fireEvent.click(screen.getByLabelText("Toggle Node A VLESS"));

		expect(onToggleCell).toHaveBeenCalledTimes(1);
	});

	it("toggles an endpoint option when its tag text is clicked", () => {
		const onToggleCellEndpoint = vi.fn();
		renderMatrix({
			onToggleCellEndpoint,
			cells: {
				"node-a": {
					vless: {
						value: "off",
						meta: {
							options: [
								{ endpointId: "ep-1", tag: "tokyo-ss", port: 443 },
								{ endpointId: "ep-2", tag: "tokyo-ss-2", port: 8443 },
							],
						},
					},
				},
			},
		});

		fireEvent.click(screen.getByText("tokyo-ss"));

		expect(onToggleCellEndpoint).toHaveBeenCalledTimes(1);
		expect(onToggleCellEndpoint).toHaveBeenCalledWith(
			"node-a",
			"vless",
			"ep-1",
			true,
		);
	});
});
