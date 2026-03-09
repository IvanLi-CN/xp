import type { Meta, StoryObj } from "@storybook/react";
import { useState } from "react";

import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";

import { AccessMatrix, type AccessMatrixCellState } from "./AccessMatrix";

const NODES = [
	{ nodeId: "n_01HnodeA", label: "node-a" },
	{ nodeId: "n_01HnodeB", label: "node-b" },
	{ nodeId: "n_01HnodeC", label: "node-c" },
	{ nodeId: "n_01HnodeD", label: "node-d" },
];

const PROTOCOLS = [
	{ protocolId: "vless_reality_vision_tcp", label: "VLESS" },
	{ protocolId: "ss2022_2022_blake3_aes_128_gcm", label: "SS2022" },
];

const REMAINING_BY_NODE: Record<string, string> = {
	n_01HnodeA: "Remaining: 8 GiB",
	n_01HnodeB: "Remaining: 4.5 GiB",
	n_01HnodeC: "Remaining: 12 GiB",
	n_01HnodeD: "Remaining: unreachable",
};

function createInitialCells(): Record<
	string,
	Record<string, AccessMatrixCellState>
> {
	return {
		n_01HnodeA: {
			vless_reality_vision_tcp: {
				value: "on",
				meta: { port: 443, tag: "node-a-vless" },
			},
			ss2022_2022_blake3_aes_128_gcm: {
				value: "off",
				meta: { port: 8443, tag: "node-a-ss" },
			},
		},
		n_01HnodeB: {
			vless_reality_vision_tcp: {
				value: "on",
				meta: { port: 443, tag: "node-b-vless" },
			},
			ss2022_2022_blake3_aes_128_gcm: {
				value: "on",
				meta: { port: 8443, tag: "node-b-ss" },
			},
		},
		n_01HnodeC: {
			vless_reality_vision_tcp: {
				value: "off",
				meta: { port: 443, tag: "node-c-vless" },
			},
			ss2022_2022_blake3_aes_128_gcm: {
				value: "off",
				meta: {
					options: [
						{ endpointId: "ep_01HSS1", tag: "node-c-ss-1", port: 8443 },
						{ endpointId: "ep_01HSS2", tag: "node-c-ss-2", port: 9443 },
					],
				},
			},
		},
		n_01HnodeD: {
			vless_reality_vision_tcp: {
				value: "disabled",
				reason: "No endpoint",
			},
			ss2022_2022_blake3_aes_128_gcm: {
				value: "off",
				meta: { port: 8443, tag: "node-d-ss" },
			},
		},
	};
}

function useDemoMatrixState() {
	const [cells, setCells] =
		useState<Record<string, Record<string, AccessMatrixCellState>>>(
			createInitialCells,
		);

	function ensureSelectedOption(nextCell: AccessMatrixCellState) {
		if (nextCell.value !== "on") return nextCell;
		const meta = nextCell.meta;
		if (!meta?.options || meta.options.length <= 1) return nextCell;
		const selectedEndpointIds = [
			...(meta.selectedEndpointIds ?? []),
			...(meta.selectedEndpointId ? [meta.selectedEndpointId] : []),
		].filter((endpointId, index, array) => array.indexOf(endpointId) === index);
		if (selectedEndpointIds.length > 0) return nextCell;
		const allEndpointIds = meta.options.map((option) => option.endpointId);
		const first = meta.options[0];
		return {
			...nextCell,
			meta: {
				...meta,
				selectedEndpointId: first.endpointId,
				selectedEndpointIds: allEndpointIds,
				port: first.port,
			},
		};
	}

	function toggleCell(nodeId: string, protocolId: string) {
		setCells((prev) => {
			const next = structuredClone(prev);
			const cell = next[nodeId]?.[protocolId];
			if (!cell || cell.value === "disabled") return prev;
			const updated: AccessMatrixCellState = {
				...cell,
				value: cell.value === "on" ? "off" : "on",
			};
			next[nodeId][protocolId] = ensureSelectedOption(updated);
			return next;
		});
	}

	function toggleRow(nodeId: string) {
		setCells((prev) => {
			const row = prev[nodeId];
			if (!row) return prev;
			const editable = Object.values(row).filter((c) => c.value !== "disabled");
			const hasAnyOn = editable.some((c) => c.value === "on");
			const next = structuredClone(prev);
			for (const [protocolId, cell] of Object.entries(next[nodeId])) {
				if (cell.value === "disabled") continue;
				next[nodeId][protocolId] = ensureSelectedOption({
					...cell,
					value: hasAnyOn ? "off" : "on",
				});
			}
			return next;
		});
	}

	function toggleColumn(protocolId: string) {
		setCells((prev) => {
			const editable: AccessMatrixCellState[] = [];
			for (const nodeId of Object.keys(prev)) {
				const cell = prev[nodeId]?.[protocolId];
				if (!cell || cell.value === "disabled") continue;
				editable.push(cell);
			}
			const hasAnyOn = editable.some((c) => c.value === "on");
			const next = structuredClone(prev);
			for (const nodeId of Object.keys(next)) {
				const cell = next[nodeId]?.[protocolId];
				if (!cell || cell.value === "disabled") continue;
				next[nodeId][protocolId] = ensureSelectedOption({
					...cell,
					value: hasAnyOn ? "off" : "on",
				});
			}
			return next;
		});
	}

	function toggleAll() {
		setCells((prev) => {
			const editable: AccessMatrixCellState[] = [];
			for (const nodeId of Object.keys(prev)) {
				for (const cell of Object.values(prev[nodeId] ?? {})) {
					if (cell.value === "disabled") continue;
					editable.push(cell);
				}
			}
			const hasAnyOn = editable.some((c) => c.value === "on");
			const next = structuredClone(prev);
			for (const nodeId of Object.keys(next)) {
				for (const protocolId of Object.keys(next[nodeId] ?? {})) {
					const cell = next[nodeId][protocolId];
					if (cell.value === "disabled") continue;
					next[nodeId][protocolId] = ensureSelectedOption({
						...cell,
						value: hasAnyOn ? "off" : "on",
					});
				}
			}
			return next;
		});
	}

	function toggleCellEndpoint(
		nodeId: string,
		protocolId: string,
		endpointId: string,
		checked: boolean,
	) {
		setCells((prev) => {
			const next = structuredClone(prev);
			const cell = next[nodeId]?.[protocolId];
			const options = cell?.meta?.options ?? [];
			const match = options.find((o) => o.endpointId === endpointId);
			if (!cell || !match) return prev;
			const existing = [
				...(cell.meta?.selectedEndpointIds ?? []),
				...(cell.meta?.selectedEndpointId
					? [cell.meta.selectedEndpointId]
					: []),
			].filter((item, index, array) => array.indexOf(item) === index);
			const nextSelected = checked
				? Array.from(new Set([...existing, endpointId]))
				: existing.filter((item) => item !== endpointId);
			const firstSelected = nextSelected[0];
			const firstMatch = options.find(
				(option) => option.endpointId === firstSelected,
			);
			next[nodeId][protocolId] = {
				...cell,
				value: nextSelected.length > 0 ? "on" : "off",
				meta: {
					...cell.meta,
					selectedEndpointId: firstSelected,
					selectedEndpointIds: nextSelected,
					port: firstMatch?.port,
				},
			};
			return next;
		});
	}

	return {
		cells,
		toggleCell,
		toggleRow,
		toggleColumn,
		toggleAll,
		toggleCellEndpoint,
	};
}

function renderNodeDetails(nodeId: string) {
	return (
		<div className="space-y-0.5">
			<div className="text-xs opacity-70">{REMAINING_BY_NODE[nodeId]}</div>
		</div>
	);
}

function HifiDemo() {
	const {
		cells,
		toggleCell,
		toggleRow,
		toggleColumn,
		toggleAll,
		toggleCellEndpoint,
	} = useDemoMatrixState();

	return (
		<div className="p-6">
			<Card className="space-y-4 p-4">
				<CardHeader className="p-0">
					<div className="flex items-baseline gap-4">
						<CardTitle className="text-sm">Matrix</CardTitle>
						<span className="text-xs text-muted-foreground">
							Batch rule: if any selected, clear; else select all (no invert)
						</span>
					</div>
				</CardHeader>
				<CardContent className="space-y-4 p-0">
					<AccessMatrix
						nodes={NODES.map((node) => ({
							...node,
							details: renderNodeDetails(node.nodeId),
						}))}
						protocols={PROTOCOLS}
						cells={cells}
						onToggleCell={toggleCell}
						onToggleRow={toggleRow}
						onToggleColumn={toggleColumn}
						onToggleAll={toggleAll}
						onToggleCellEndpoint={toggleCellEndpoint}
					/>
					<p className="text-xs text-muted-foreground">
						Tip: header checkboxes can show indeterminate state, but clicking
						never inverts.
					</p>
				</CardContent>
			</Card>
		</div>
	);
}

const meta: Meta<typeof HifiDemo> = {
	title: "Design/Components/AccessMatrix",
	component: HifiDemo,
	tags: ["autodocs", "coverage-ui"],
	parameters: {
		layout: "fullscreen",
	},
};

export default meta;

type Story = StoryObj<typeof meta>;

export const Hifi: Story = {};
