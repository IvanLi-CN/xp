import type { Meta, StoryObj } from "@storybook/react";
import { useMemo, useState } from "react";

import {
	GrantAccessMatrix,
	type GrantAccessMatrixCellState,
} from "./GrantAccessMatrix";
import { NodeQuotaEditor, type NodeQuotaEditorValue } from "./NodeQuotaEditor";

function HifiDemo() {
	const nodes = useMemo(
		() => [
			{ nodeId: "n_01HnodeA", label: "node-a" },
			{ nodeId: "n_01HnodeB", label: "node-b" },
			{ nodeId: "n_01HnodeC", label: "node-c" },
			{ nodeId: "n_01HnodeD", label: "node-d" },
		],
		[],
	);

	const protocols = useMemo(
		() => [
			{ protocolId: "vless_reality_vision_tcp", label: "VLESS" },
			{ protocolId: "ss2022_2022_blake3_aes_128_gcm", label: "SS2022" },
		],
		[],
	);

	const [cells, setCells] = useState<
		Record<string, Record<string, GrantAccessMatrixCellState>>
	>(() => ({
		n_01HnodeA: {
			vless_reality_vision_tcp: {
				value: "on",
				meta: { port: 443, grantId: "g-01" },
			},
			ss2022_2022_blake3_aes_128_gcm: {
				value: "off",
				meta: { port: 8443 },
			},
		},
		n_01HnodeB: {
			vless_reality_vision_tcp: {
				value: "on",
				meta: { port: 443, grantId: "g-02" },
			},
			ss2022_2022_blake3_aes_128_gcm: {
				value: "on",
				meta: { port: 8443, grantId: "g-03" },
			},
		},
		n_01HnodeC: {
			vless_reality_vision_tcp: {
				value: "off",
				meta: { port: 443 },
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
				meta: { port: 8443 },
			},
		},
	}));

	const [nodeQuotas, setNodeQuotas] = useState<
		Record<string, NodeQuotaEditorValue>
	>(() => ({
		n_01HnodeA: 10 * 2 ** 30,
		n_01HnodeB: "mixed",
		n_01HnodeC: 0,
		n_01HnodeD: 512 * 2 ** 20,
	}));

	function ensureSelectedOption(nextCell: GrantAccessMatrixCellState) {
		if (nextCell.value !== "on") return nextCell;
		const meta = nextCell.meta;
		if (!meta?.options || meta.options.length <= 1) return nextCell;
		if (meta.selectedEndpointId) return nextCell;
		const first = meta.options[0];
		return {
			...nextCell,
			meta: {
				...meta,
				selectedEndpointId: first.endpointId,
				port: first.port,
			},
		};
	}

	function toggleCell(nodeId: string, protocolId: string) {
		setCells((prev) => {
			const next = structuredClone(prev);
			const cell = next[nodeId]?.[protocolId];
			if (!cell || cell.value === "disabled") return prev;
			const updated: GrantAccessMatrixCellState = {
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
			const editable: GrantAccessMatrixCellState[] = [];
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
			const editable: GrantAccessMatrixCellState[] = [];
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

	function selectEndpoint(
		nodeId: string,
		protocolId: string,
		endpointId: string,
	) {
		setCells((prev) => {
			const next = structuredClone(prev);
			const cell = next[nodeId]?.[protocolId];
			const options = cell?.meta?.options ?? [];
			const match = options.find((o) => o.endpointId === endpointId);
			if (!cell || !match) return prev;
			next[nodeId][protocolId] = {
				...cell,
				value: "on",
				meta: {
					...cell.meta,
					selectedEndpointId: match.endpointId,
					port: match.port,
				},
			};
			return next;
		});
	}

	return (
		<div className="p-6">
			<div className="rounded-box border border-base-200 bg-base-100 p-4 space-y-4">
				<div className="flex items-baseline gap-4">
					<span className="text-sm font-semibold">Matrix</span>
					<span className="text-xs opacity-60">
						Batch rule: if any selected, clear; else select all (no invert)
					</span>
				</div>
				<GrantAccessMatrix
					nodes={nodes.map((n) => ({
						...n,
						details: (
							<NodeQuotaEditor
								value={nodeQuotas[n.nodeId] ?? 0}
								onApply={async (nextBytes) => {
									setNodeQuotas((prev) => ({ ...prev, [n.nodeId]: nextBytes }));
								}}
							/>
						),
					}))}
					protocols={protocols}
					cells={cells}
					onToggleCell={toggleCell}
					onToggleRow={toggleRow}
					onToggleColumn={toggleColumn}
					onToggleAll={toggleAll}
					onSelectCellEndpoint={selectEndpoint}
				/>
				<p className="text-xs opacity-60">
					Tip: header checkboxes can show indeterminate state, but clicking
					never inverts.
				</p>
			</div>
		</div>
	);
}

const meta: Meta<typeof HifiDemo> = {
	title: "Design/Components/GrantAccessMatrix",
	component: HifiDemo,
	parameters: {
		layout: "fullscreen",
	},
};

export default meta;

type Story = StoryObj<typeof HifiDemo>;

export const Hifi: Story = {};
