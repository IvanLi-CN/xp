import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";

import { cn } from "@/lib/utils";

import { Icon } from "./Icon";
import { tableClass } from "./ui-helpers";

export type AccessMatrixNode = {
	nodeId: string;
	label: string;
	details?: ReactNode;
};

export type AccessMatrixProtocol = {
	protocolId: string;
	label: string;
};

export type AccessMatrixCellMeta = {
	endpointId?: string;
	tag?: string;
	port?: number;
	options?: Array<{
		endpointId: string;
		tag: string;
		port: number;
	}>;
	selectedEndpointId?: string;
	selectedEndpointIds?: string[];
};

export type AccessMatrixCellState = {
	value: "on" | "off" | "disabled";
	reason?: string;
	meta?: AccessMatrixCellMeta;
};

type CheckboxState = {
	checked: boolean;
	indeterminate: boolean;
	disabled: boolean;
};

type CellSelectionState = {
	checked: boolean;
	indeterminate: boolean;
	disabled: boolean;
};

function collectSelectedEndpointIds(meta?: AccessMatrixCellMeta): string[] {
	if (!meta) return [];
	return [
		...(meta.selectedEndpointIds ?? []),
		...(meta.selectedEndpointId ? [meta.selectedEndpointId] : []),
	].filter((endpointId, index, array) => array.indexOf(endpointId) === index);
}

function getCellSelectionState(
	cell: AccessMatrixCellState,
): CellSelectionState {
	if (cell.value === "disabled") {
		return {
			checked: false,
			indeterminate: false,
			disabled: true,
		};
	}

	const options = cell.meta?.options ?? [];
	if (options.length > 1) {
		const optionIds = new Set(options.map((option) => option.endpointId));
		const selectedCount = collectSelectedEndpointIds(cell.meta).filter(
			(endpointId) => optionIds.has(endpointId),
		).length;
		return {
			checked: selectedCount === options.length,
			indeterminate: selectedCount > 0 && selectedCount < options.length,
			disabled: false,
		};
	}

	return {
		checked:
			cell.value === "on" || collectSelectedEndpointIds(cell.meta).length > 0,
		indeterminate: false,
		disabled: false,
	};
}

function aggregateCheckboxState(states: CellSelectionState[]): CheckboxState {
	const editable = states.filter((state) => !state.disabled);
	const total = editable.length;
	const checkedCount = editable.filter((state) => state.checked).length;
	const hasIndeterminate = editable.some((state) => state.indeterminate);
	const checked = total > 0 && checkedCount === total;
	return {
		disabled: total === 0,
		checked,
		indeterminate:
			total > 0 && !checked && (checkedCount > 0 || hasIndeterminate),
	};
}

function IndeterminateCheckbox(props: {
	checked: boolean;
	indeterminate: boolean;
	disabled?: boolean;
	ariaLabel: string;
	onChange?: () => void;
}) {
	const { checked, indeterminate, disabled, ariaLabel, onChange } = props;
	const ref = useRef<HTMLInputElement | null>(null);
	useEffect(() => {
		if (!ref.current) return;
		ref.current.indeterminate = indeterminate && !checked;
	}, [checked, indeterminate]);

	return (
		<input
			ref={ref}
			type="checkbox"
			className="size-4 shrink-0 rounded border-input bg-background accent-primary shadow-xs focus-visible:ring-[3px] focus-visible:ring-ring/20 disabled:cursor-not-allowed disabled:opacity-50"
			checked={checked}
			disabled={disabled}
			aria-label={ariaLabel}
			onChange={() => onChange?.()}
		/>
	);
}

export type AccessMatrixProps = {
	nodes: AccessMatrixNode[];
	protocols: AccessMatrixProtocol[];
	disabled?: boolean;
	// nodeId -> protocolId -> state
	cells: Record<string, Record<string, AccessMatrixCellState>>;
	onToggleCell?: (nodeId: string, protocolId: string) => void;
	onToggleRow?: (nodeId: string) => void;
	onToggleColumn?: (protocolId: string) => void;
	onToggleAll?: () => void;
	onToggleCellEndpoint?: (
		nodeId: string,
		protocolId: string,
		endpointId: string,
		checked: boolean,
	) => void;
};

export function AccessMatrix(props: AccessMatrixProps) {
	const {
		nodes,
		protocols,
		disabled,
		cells,
		onToggleAll,
		onToggleCell,
		onToggleColumn,
		onToggleRow,
		onToggleCellEndpoint,
	} = props;

	const allState = useMemo(() => {
		const states: CellSelectionState[] = [];
		for (const node of nodes) {
			for (const protocol of protocols) {
				const cell = cells[node.nodeId]?.[protocol.protocolId] ?? {
					value: "off",
				};
				states.push(getCellSelectionState(cell));
			}
		}
		return aggregateCheckboxState(states);
	}, [cells, nodes, protocols]);

	const columnStates = useMemo(() => {
		const out = new Map<string, CheckboxState>();
		for (const protocol of protocols) {
			const states: CellSelectionState[] = [];
			for (const node of nodes) {
				const cell = cells[node.nodeId]?.[protocol.protocolId] ?? {
					value: "off",
				};
				states.push(getCellSelectionState(cell));
			}
			out.set(protocol.protocolId, aggregateCheckboxState(states));
		}
		return out;
	}, [cells, nodes, protocols]);

	const rowStates = useMemo(() => {
		const out = new Map<string, CheckboxState>();
		for (const node of nodes) {
			const states: CellSelectionState[] = [];
			for (const protocol of protocols) {
				const cell = cells[node.nodeId]?.[protocol.protocolId] ?? {
					value: "off",
				};
				states.push(getCellSelectionState(cell));
			}
			out.set(node.nodeId, aggregateCheckboxState(states));
		}
		return out;
	}, [cells, nodes, protocols]);
	const protocolColumnWidth =
		protocols.length > 0
			? `calc((100% - 16.5rem) / ${protocols.length})`
			: undefined;
	const matrixMinWidthRem = 16.5 + protocols.length * 11;

	return (
		<div className="xp-table-wrap">
			<table
				className={tableClass(true, "table-fixed w-full")}
				style={{ minWidth: `${matrixMinWidthRem}rem` }}
			>
				<colgroup>
					<col style={{ width: "2.5rem" }} />
					<col style={{ width: "14rem" }} />
					{protocols.map((protocol) => (
						<col
							key={`col-${protocol.protocolId}`}
							style={
								protocolColumnWidth ? { width: protocolColumnWidth } : undefined
							}
						/>
					))}
				</colgroup>
				<thead>
					<tr>
						<th className="w-8">
							<IndeterminateCheckbox
								checked={allState.checked}
								indeterminate={allState.indeterminate}
								disabled={disabled || allState.disabled}
								ariaLabel="Toggle all"
								onChange={() => onToggleAll?.()}
							/>
						</th>
						<th>Node</th>
						{protocols.map((protocol) => {
							const state = columnStates.get(protocol.protocolId) ?? {
								checked: false,
								indeterminate: false,
								disabled: true,
							};
							return (
								<th key={protocol.protocolId}>
									<div className="flex items-center gap-2">
										<IndeterminateCheckbox
											checked={state.checked}
											indeterminate={state.indeterminate}
											disabled={disabled || state.disabled}
											ariaLabel={`Toggle ${protocol.label}`}
											onChange={() => onToggleColumn?.(protocol.protocolId)}
										/>
										<span>{protocol.label}</span>
									</div>
								</th>
							);
						})}
					</tr>
				</thead>
				<tbody>
					{nodes.map((node) => {
						const rowState = rowStates.get(node.nodeId) ?? {
							checked: false,
							indeterminate: false,
							disabled: true,
						};
						return (
							<tr key={node.nodeId}>
								<td className="w-8">
									<IndeterminateCheckbox
										checked={rowState.checked}
										indeterminate={rowState.indeterminate}
										disabled={disabled || rowState.disabled}
										ariaLabel={`Toggle row ${node.label}`}
										onChange={() => onToggleRow?.(node.nodeId)}
									/>
								</td>
								<td className="space-y-0.5">
									<div className="font-medium">{node.label}</div>
									{node.details ? node.details : null}
								</td>
								{protocols.map((protocol) => {
									const cell = cells[node.nodeId]?.[protocol.protocolId] ?? {
										value: "off",
									};
									return (
										<td key={`${node.nodeId}::${protocol.protocolId}`}>
											{cell.value === "disabled" ? (
												<span className="text-xs text-muted-foreground">
													{cell.reason ?? "Disabled"}
												</span>
											) : (
												<AccessMatrixCellLabel
													nodeId={node.nodeId}
													nodeLabel={node.label}
													protocolId={protocol.protocolId}
													protocolLabel={protocol.label}
													cell={cell}
													disabled={disabled}
													onToggleCell={() =>
														onToggleCell?.(node.nodeId, protocol.protocolId)
													}
													onToggleCellEndpoint={onToggleCellEndpoint}
												/>
											)}
										</td>
									);
								})}
							</tr>
						);
					})}
				</tbody>
			</table>
		</div>
	);
}

function shortId(id: string): string {
	return id.length <= 8 ? id : `${id.slice(0, 4)}..${id.slice(-4)}`;
}

function AccessMatrixCellLabel(props: {
	nodeId: string;
	nodeLabel: string;
	protocolId: string;
	protocolLabel: string;
	cell: AccessMatrixCellState;
	disabled?: boolean;
	onToggleCell?: () => void;
	onToggleCellEndpoint?: (
		nodeId: string,
		protocolId: string,
		endpointId: string,
		checked: boolean,
	) => void;
}) {
	const {
		nodeId,
		nodeLabel,
		protocolId,
		protocolLabel,
		cell,
		disabled,
		onToggleCell,
		onToggleCellEndpoint,
	} = props;
	const [expanded, setExpanded] = useState(true);
	const meta = cell.meta;
	if (!meta) return null;

	const options = meta.options ?? [];
	if (options.length > 1) {
		const optionIds = new Set(options.map((option) => option.endpointId));
		const selectedEndpointIds = collectSelectedEndpointIds(meta).filter(
			(endpointId) => optionIds.has(endpointId),
		);
		const selectedCount = selectedEndpointIds.length;
		const allSelected = selectedCount === options.length;
		const partiallySelected =
			selectedCount > 0 && selectedCount < options.length;
		const selectedLabel =
			selectedCount === 0
				? "none selected"
				: `selected ${selectedCount}/${options.length}`;
		function toggleAll(checked: boolean) {
			for (const option of options) {
				onToggleCellEndpoint?.(nodeId, protocolId, option.endpointId, checked);
			}
		}

		return (
			<div className="grid grid-cols-[1rem_minmax(0,1fr)] items-center gap-x-2 gap-y-0.5 px-1 py-0.5">
				<div className="self-center -mt-px">
					<IndeterminateCheckbox
						checked={allSelected}
						indeterminate={partiallySelected}
						disabled={disabled}
						ariaLabel={`Toggle all endpoints for ${nodeId} ${protocolId}`}
						onChange={() => toggleAll(!allSelected)}
					/>
				</div>
				<button
					type="button"
					className="flex min-h-6 min-w-0 items-center gap-1 text-left opacity-80 transition-opacity hover:opacity-100"
					aria-expanded={expanded}
					aria-label={`Toggle endpoint tree for ${nodeId} ${protocolId}`}
					onClick={() => setExpanded((value) => !value)}
				>
					<span className="relative inline-flex size-4 shrink-0 items-center justify-center">
						<Icon
							name="tabler:folder"
							size={16}
							className={cn(
								"absolute transition-all duration-200 ease-out",
								expanded
									? "-rotate-6 scale-90 opacity-0"
									: "rotate-0 scale-100 opacity-80",
							)}
						/>
						<Icon
							name="tabler:folder-open"
							size={16}
							className={cn(
								"absolute transition-all duration-200 ease-out",
								expanded
									? "rotate-0 scale-100 opacity-90"
									: "scale-90 rotate-6 opacity-0",
							)}
						/>
					</span>
					<span className="truncate font-mono text-sm font-medium">
						endpoint tree
					</span>
					<span className="ml-auto truncate font-mono text-xs text-muted-foreground">
						{selectedLabel}
					</span>
				</button>
				{expanded ? (
					<ul
						role="tree"
						aria-label={`Endpoint options for ${nodeId} ${protocolId}`}
						className="col-start-2 space-y-1"
					>
						{options.map((opt, index) => {
							const selected = selectedEndpointIds.includes(opt.endpointId);
							const isFirst = index === 0;
							const isLast = index === options.length - 1;
							return (
								<li
									key={opt.endpointId}
									role="treeitem"
									aria-selected={selected}
									className="relative min-w-0 pl-4"
								>
									{!isFirst ? (
										<span
											aria-hidden="true"
											className="pointer-events-none absolute left-2 top-0 h-1/2 w-px bg-foreground/20"
										/>
									) : null}
									{!isLast ? (
										<span
											aria-hidden="true"
											className="pointer-events-none absolute left-2 top-1/2 h-1/2 w-px bg-foreground/20"
										/>
									) : null}
									<span
										aria-hidden="true"
										className="pointer-events-none absolute left-2 top-1/2 h-px w-2 -translate-y-1/2 bg-foreground/20"
									/>
									<label
										className={cn(
											"flex items-center gap-1 rounded-lg px-1 py-0.5 transition-colors",
											disabled ? "opacity-60" : "cursor-pointer",
											selected ? "bg-primary/10" : "hover:bg-muted/60",
										)}
									>
										<input
											type="checkbox"
											className="size-4 shrink-0 rounded border-input bg-background accent-primary shadow-xs focus-visible:ring-[3px] focus-visible:ring-ring/20 disabled:cursor-not-allowed disabled:opacity-50"
											checked={selected}
											disabled={disabled}
											aria-label={`Select endpoint ${opt.tag} for ${nodeId} ${protocolId}`}
											onChange={(event) =>
												onToggleCellEndpoint?.(
													nodeId,
													protocolId,
													opt.endpointId,
													event.target.checked,
												)
											}
										/>
										<span className="min-w-0 flex-1 truncate font-mono text-xs">
											{opt.tag}
										</span>
										<span className="font-mono text-xs text-muted-foreground">
											:{opt.port}
										</span>
										<span className="font-mono text-[10px] text-muted-foreground">
											{shortId(opt.endpointId)}
										</span>
									</label>
								</li>
							);
						})}
					</ul>
				) : null}
				{selectedCount === 1 ? (
					<span className="col-start-2 block truncate font-mono text-xs text-muted-foreground">
						port {meta.port ?? "?"} - endpoint {shortId(selectedEndpointIds[0])}
					</span>
				) : selectedCount > 1 ? (
					<span className="col-start-2 block truncate font-mono text-xs text-muted-foreground">
						{selectedCount} endpoints selected
					</span>
				) : null}
			</div>
		);
	}

	const selectedEndpointIds = collectSelectedEndpointIds(meta);
	const selected = cell.value === "on" || selectedEndpointIds.length > 0;

	return (
		<label
			className={cn(
				"flex items-center gap-2 rounded-lg px-1 py-0.5 transition-colors",
				disabled ? "opacity-60" : "cursor-pointer hover:bg-muted/60",
			)}
		>
			<input
				type="checkbox"
				className="size-4 shrink-0 rounded border-input bg-background accent-primary shadow-xs focus-visible:ring-[3px] focus-visible:ring-ring/20 disabled:cursor-not-allowed disabled:opacity-50"
				checked={selected}
				disabled={disabled}
				aria-label={`Toggle ${nodeLabel} ${protocolLabel}`}
				onChange={() => onToggleCell?.()}
			/>
			<span className="block truncate font-mono text-sm text-muted-foreground">
				port {meta.port ?? "?"}
			</span>
			{meta.endpointId ? (
				<span className="font-mono text-[10px] text-muted-foreground">
					{shortId(meta.endpointId)}
				</span>
			) : null}
		</label>
	);
}
