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
			nodes={[{ nodeId: fixtureCatalog.nodeId.fixture106(), label: "Node A" }]}
			protocols={[{ protocolId: "vless", label: "VLESS" }]}
			cells={
				args?.cells ?? {
					[fixtureCatalog.nodeId.fixture106()]: {
						vless: {
							value: "off",
							meta: {
								port: fixtureCatalog.endpoint.port443(),
								endpointId: fixtureCatalog.endpointId.fixture138(),
							},
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
			fixtureCatalog.nodeId.fixture106(),
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
				[fixtureCatalog.nodeId.fixture106()]: {
					vless: {
						value: "off",
						meta: {
							options: [
								{
									endpointId: fixtureCatalog.endpointId.fixture285(),
									tag: fixtureCatalog.endpointTag.fixture141(),
									port: fixtureCatalog.endpoint.port443(),
								},
								{
									endpointId: fixtureCatalog.endpointId.fixture286(),
									tag: fixtureCatalog.endpointTag.fixture256(),
									port: fixtureCatalog.endpoint.port8443(),
								},
							],
						},
					},
				},
			},
		});

		fireEvent.click(screen.getByText(fixtureCatalog.endpointTag.fixture141()));

		expect(onToggleCellEndpoint).toHaveBeenCalledTimes(1);
		expect(onToggleCellEndpoint).toHaveBeenCalledWith(
			fixtureCatalog.nodeId.fixture106(),
			"vless",
			fixtureCatalog.endpointId.fixture285(),
			true,
		);
	});
});
