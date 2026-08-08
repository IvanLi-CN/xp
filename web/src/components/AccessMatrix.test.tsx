import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { fixtureCatalog } from "../fixture-policy/catalog";

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
			nodes={[{ nodeId: fixtureCatalog.slotString.s106(), label: "Node A" }]}
			protocols={[{ protocolId: "vless", label: "VLESS" }]}
			cells={
				args?.cells ?? {
					[fixtureCatalog.slotString.s106()]: {
						vless: {
							value: "off",
							meta: { port: 443, endpointId: fixtureCatalog.slotString.s138() },
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
		expect(onToggleCell).toHaveBeenCalledWith(
			fixtureCatalog.slotString.s106(),
			"vless",
		);
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
				[fixtureCatalog.slotString.s106()]: {
					vless: {
						value: "off",
						meta: {
							options: [
								{
									endpointId: fixtureCatalog.slotString.s285(),
									tag: fixtureCatalog.slotString.s141(),
									port: 443,
								},
								{
									endpointId: fixtureCatalog.slotString.s286(),
									tag: fixtureCatalog.slotString.s256(),
									port: 8443,
								},
							],
						},
					},
				},
			},
		});

		fireEvent.click(screen.getByText(fixtureCatalog.slotString.s141()));

		expect(onToggleCellEndpoint).toHaveBeenCalledTimes(1);
		expect(onToggleCellEndpoint).toHaveBeenCalledWith(
			fixtureCatalog.slotString.s106(),
			"vless",
			fixtureCatalog.slotString.s285(),
			true,
		);
	});
});
