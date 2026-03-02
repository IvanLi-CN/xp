import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { Icon } from "./Icon";

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

function useCheckboxState(
	values: Array<"on" | "off" | "disabled">,
): CheckboxState {
	const editable = values.filter((v) => v !== "disabled");
	const onCount = editable.filter((v) => v === "on").length;
	const total = editable.length;
	return {
		disabled: total === 0,
		checked: total > 0 && onCount === total,
		indeterminate: total > 0 && onCount > 0 && onCount < total,
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
			className="checkbox checkbox-xs checkbox-primary rounded"
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

	const allValues = useMemo(() => {
		const values: Array<"on" | "off" | "disabled"> = [];
		for (const node of nodes) {
			for (const protocol of protocols) {
				values.push(cells[node.nodeId]?.[protocol.protocolId]?.value ?? "off");
			}
		}
		return values;
	}, [cells, nodes, protocols]);
	const allState = useCheckboxState(allValues);

	const columnStates = useMemo(() => {
		const out = new Map<string, CheckboxState>();
		for (const protocol of protocols) {
			const values: Array<"on" | "off" | "disabled"> = [];
			for (const node of nodes) {
				values.push(cells[node.nodeId]?.[protocol.protocolId]?.value ?? "off");
			}
			out.set(protocol.protocolId, useCheckboxState(values));
		}
		return out;
	}, [cells, nodes, protocols]);

	const rowStates = useMemo(() => {
		const out = new Map<string, CheckboxState>();
		for (const node of nodes) {
			const values: Array<"on" | "off" | "disabled"> = [];
			for (const protocol of protocols) {
				values.push(cells[node.nodeId]?.[protocol.protocolId]?.value ?? "off");
			}
			out.set(node.nodeId, useCheckboxState(values));
		}
		return out;
	}, [cells, nodes, protocols]);
	const protocolColumnWidth =
		protocols.length > 0
			? `calc((100% - 16.5rem) / ${protocols.length})`
			: undefined;
	const matrixMinWidthRem = 16.5 + protocols.length * 11;

	return (
		<div className="overflow-x-auto">
			<table
				className="table table-zebra table-sm table-fixed w-full"
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
												<span className="text-xs opacity-60">
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
		const selectedEndpointIds = [
			...(meta.selectedEndpointIds ?? []),
			...(meta.selectedEndpointId ? [meta.selectedEndpointId] : []),
		].filter((endpointId, index, array) => array.indexOf(endpointId) === index);
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
					className="flex min-h-6 min-w-0 items-center gap-1 text-left opacity-80 hover:opacity-100"
					aria-expanded={expanded}
					aria-label={`Toggle endpoint tree for ${nodeId} ${protocolId}`}
					onClick={() => setExpanded((value) => !value)}
				>
					<span className="relative inline-flex size-4 shrink-0 items-center justify-center">
						<Icon
							name="tabler:folder"
							size={16}
							className={[
								"absolute transition-all duration-200 ease-out",
								expanded
									? "opacity-0 scale-90 -rotate-6"
									: "opacity-80 scale-100 rotate-0",
							]
								.filter(Boolean)
								.join(" ")}
						/>
						<Icon
							name="tabler:folder-open"
							size={16}
							className={[
								"absolute transition-all duration-200 ease-out",
								expanded
									? "opacity-90 scale-100 rotate-0"
									: "opacity-0 scale-90 rotate-6",
							]
								.filter(Boolean)
								.join(" ")}
						/>
					</span>
					<span className="font-mono text-sm font-medium truncate">
						endpoint tree
					</span>
					<span className="ml-auto font-mono text-xs opacity-60 truncate">
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
											className="pointer-events-none absolute left-2 top-0 h-1/2 w-px bg-base-content opacity-25"
										/>
									) : null}
									{!isLast ? (
										<span
											aria-hidden="true"
											className="pointer-events-none absolute left-2 top-1/2 h-1/2 w-px bg-base-content opacity-25"
										/>
									) : null}
									<span
										aria-hidden="true"
										className="pointer-events-none absolute left-2 top-1/2 h-px w-2 -translate-y-1/2 bg-base-content opacity-25"
									/>
									<label
										className={[
											"flex items-center gap-1 rounded px-1 py-0.5",
											disabled ? "opacity-60" : "cursor-pointer",
											selected ? "bg-primary/15" : "hover:bg-base-200/60",
										]
											.filter(Boolean)
											.join(" ")}
									>
										<input
											type="checkbox"
											className="checkbox checkbox-xs checkbox-primary rounded"
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
										<span className="min-w-0 flex-1 font-mono text-xs truncate">
											{opt.tag}
										</span>
										<span className="font-mono text-xs opacity-70">
											:{opt.port}
										</span>
										<span className="font-mono text-[10px] opacity-60">
											{shortId(opt.endpointId)}
										</span>
									</label>
								</li>
							);
						})}
					</ul>
				) : null}
				{selectedCount === 1 ? (
					<span className="col-start-2 font-mono text-xs opacity-60 truncate block">
						port {meta.port ?? "?"} - endpoint {shortId(selectedEndpointIds[0])}
					</span>
				) : selectedCount > 1 ? (
					<span className="col-start-2 font-mono text-xs opacity-60 truncate block">
						{selectedCount} endpoints selected
					</span>
				) : null}
			</div>
		);
	}

	const selectedEndpointIds = [
		...(meta.selectedEndpointIds ?? []),
		...(meta.selectedEndpointId ? [meta.selectedEndpointId] : []),
	].filter((endpointId, index, array) => array.indexOf(endpointId) === index);
	const selected = cell.value === "on" || selectedEndpointIds.length > 0;

	return (
		<label
			className={[
				"flex items-center gap-2 rounded px-1 py-0.5",
				disabled ? "opacity-60" : "cursor-pointer hover:bg-base-200/60",
			]
				.filter(Boolean)
				.join(" ")}
		>
			<input
				type="checkbox"
				className="checkbox checkbox-xs checkbox-primary rounded"
				checked={selected}
				disabled={disabled}
				aria-label={`Toggle ${nodeLabel} ${protocolLabel}`}
				onChange={() => onToggleCell?.()}
			/>
			<span className="font-mono text-sm opacity-70 truncate block">
				port {meta.port ?? "?"}
			</span>
			{meta.endpointId ? (
				<span className="font-mono text-[10px] opacity-60">
					{shortId(meta.endpointId)}
				</span>
			) : null}
		</label>
	);
}
