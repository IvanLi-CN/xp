import { type ReactNode, useEffect, useMemo, useRef } from "react";

export type GrantAccessMatrixNode = {
	nodeId: string;
	label: string;
	details?: ReactNode;
};

export type GrantAccessMatrixProtocol = {
	protocolId: string;
	label: string;
};

export type GrantAccessMatrixCellMeta = {
	endpointId?: string;
	grantId?: string;
	tag?: string;
	port?: number;
	options?: Array<{
		endpointId: string;
		tag: string;
		port: number;
	}>;
	selectedEndpointId?: string;
};

export type GrantAccessMatrixCellState = {
	value: "on" | "off" | "disabled";
	reason?: string;
	meta?: GrantAccessMatrixCellMeta;
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

export type GrantAccessMatrixProps = {
	nodes: GrantAccessMatrixNode[];
	protocols: GrantAccessMatrixProtocol[];
	// nodeId -> protocolId -> state
	cells: Record<string, Record<string, GrantAccessMatrixCellState>>;
	onToggleCell?: (nodeId: string, protocolId: string) => void;
	onToggleRow?: (nodeId: string) => void;
	onToggleColumn?: (protocolId: string) => void;
	onToggleAll?: () => void;
	onSelectCellEndpoint?: (
		nodeId: string,
		protocolId: string,
		endpointId: string,
	) => void;
};

export function GrantAccessMatrix(props: GrantAccessMatrixProps) {
	const {
		nodes,
		protocols,
		cells,
		onToggleAll,
		onToggleCell,
		onToggleColumn,
		onToggleRow,
		onSelectCellEndpoint,
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
		const map = new Map<string, CheckboxState>();
		for (const protocol of protocols) {
			const values: Array<"on" | "off" | "disabled"> = [];
			for (const node of nodes) {
				values.push(cells[node.nodeId]?.[protocol.protocolId]?.value ?? "off");
			}
			map.set(protocol.protocolId, useCheckboxState(values));
		}
		return map;
	}, [cells, nodes, protocols]);

	const rowStates = useMemo(() => {
		const map = new Map<string, CheckboxState>();
		for (const node of nodes) {
			const values: Array<"on" | "off" | "disabled"> = [];
			for (const protocol of protocols) {
				values.push(cells[node.nodeId]?.[protocol.protocolId]?.value ?? "off");
			}
			map.set(node.nodeId, useCheckboxState(values));
		}
		return map;
	}, [cells, nodes, protocols]);

	return (
		<div className="overflow-auto rounded-box border border-base-200">
			<table className="table table-fixed">
				<thead>
					<tr className="bg-base-200/50">
						<th className="sticky left-0 top-0 z-20 bg-base-200/50 w-56">
							<div className="flex items-center gap-3">
								<IndeterminateCheckbox
									checked={allState.checked}
									indeterminate={allState.indeterminate}
									disabled={allState.disabled}
									ariaLabel="Toggle all nodes and protocols"
									onChange={() => onToggleAll?.()}
								/>
								<span className="font-medium">All nodes</span>
							</div>
						</th>
						{protocols.map((protocol) => {
							const state = columnStates.get(protocol.protocolId) ?? {
								checked: false,
								indeterminate: false,
								disabled: true,
							};
							return (
								<th
									key={protocol.protocolId}
									className="sticky top-0 z-10 bg-base-200/50"
								>
									<div className="flex items-center gap-3">
										<IndeterminateCheckbox
											checked={state.checked}
											indeterminate={state.indeterminate}
											disabled={state.disabled}
											ariaLabel={`Toggle protocol ${protocol.label}`}
											onChange={() => onToggleColumn?.(protocol.protocolId)}
										/>
										<div className="min-w-0">
											<div className="flex items-baseline gap-2 min-w-0">
												<span className="font-medium">{protocol.label}</span>
												<span className="font-mono text-xs opacity-60 truncate block">
													{protocol.protocolId}
												</span>
											</div>
										</div>
									</div>
								</th>
							);
						})}
					</tr>
				</thead>
				<tbody>
					{nodes.map((node, idx) => {
						const rowState = rowStates.get(node.nodeId) ?? {
							checked: false,
							indeterminate: false,
							disabled: true,
						};
						const zebra = idx % 2 === 1 ? "bg-base-200/15" : "";
						return (
							<tr key={node.nodeId} className={zebra}>
								<th className="sticky left-0 z-10 bg-base-100 w-56">
									<div className="flex items-center gap-3">
										<IndeterminateCheckbox
											checked={rowState.checked}
											indeterminate={rowState.indeterminate}
											disabled={rowState.disabled}
											ariaLabel={`Toggle node ${node.label}`}
											onChange={() => onToggleRow?.(node.nodeId)}
										/>
										<div className="min-w-0">
											<div className="font-medium">{node.label}</div>
											<div className="font-mono text-xs opacity-60 truncate">
												{node.nodeId}
											</div>
											{node.details ? (
												<div className="min-w-0">{node.details}</div>
											) : null}
										</div>
									</div>
								</th>
								{protocols.map((protocol) => {
									const cell =
										cells[node.nodeId]?.[protocol.protocolId] ??
										({ value: "off" } as const);
									const ariaLabel = `Toggle ${node.label} ${protocol.label}`;
									return (
										<td key={protocol.protocolId}>
											<div
												className={[
													"flex items-center gap-3 rounded-box py-2",
													cell.value === "disabled"
														? "bg-base-200/35 opacity-70"
														: "hover:bg-base-200/25",
												].join(" ")}
											>
												<input
													type="checkbox"
													className="checkbox checkbox-xs checkbox-primary rounded"
													checked={cell.value === "on"}
													disabled={cell.value === "disabled"}
													aria-label={ariaLabel}
													onChange={() =>
														onToggleCell?.(node.nodeId, protocol.protocolId)
													}
												/>
												<div className="min-w-0 flex-1">
													<CellContent
														cell={cell}
														nodeId={node.nodeId}
														protocolId={protocol.protocolId}
														onSelectCellEndpoint={onSelectCellEndpoint}
													/>
												</div>
											</div>
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

function shortId(value: string): string {
	if (value.length <= 10) return value;
	return `${value.slice(0, 4)}...${value.slice(-4)}`;
}

function CellContent(props: {
	cell: GrantAccessMatrixCellState;
	nodeId: string;
	protocolId: string;
	onSelectCellEndpoint?: (
		nodeId: string,
		protocolId: string,
		endpointId: string,
	) => void;
}) {
	const { cell, nodeId, protocolId, onSelectCellEndpoint } = props;

	if (cell.value === "disabled") {
		return (
			<div className="flex items-center justify-between gap-3 min-w-0">
				<span className="text-sm opacity-70 truncate block">
					{cell.reason ?? "Unavailable"}
				</span>
				<span className="font-mono text-xs opacity-50">(disabled)</span>
			</div>
		);
	}

	const meta = cell.meta ?? {};
	const options = meta.options ?? [];
	const showChooser = options.length > 1 && onSelectCellEndpoint;

	if (showChooser) {
		return (
			<div className="flex items-center justify-between gap-3 min-w-0">
				<div className="min-w-0">
					{meta.selectedEndpointId ? (
						<span className="font-mono text-sm opacity-70 truncate block">
							port {meta.port ?? "?"} - endpoint{" "}
							{shortId(meta.selectedEndpointId)}
						</span>
					) : (
						<span className="font-mono text-sm opacity-70 truncate block">
							multiple endpoints
						</span>
					)}
				</div>
				<div className="dropdown dropdown-end">
					<button type="button" className="btn btn-ghost btn-xs">
						Choose ({options.length})
					</button>
					<ul className="dropdown-content menu z-[1] w-64 rounded-box border border-base-200 bg-base-100 shadow">
						{options.map((opt) => (
							<li key={opt.endpointId}>
								<button
									type="button"
									className="justify-between"
									onClick={() =>
										onSelectCellEndpoint?.(nodeId, protocolId, opt.endpointId)
									}
								>
									<span className="font-mono text-xs">
										port {opt.port} - {opt.tag}
									</span>
									<span className="font-mono text-xs opacity-60">
										{shortId(opt.endpointId)}
									</span>
								</button>
							</li>
						))}
					</ul>
				</div>
			</div>
		);
	}

	const bits: string[] = [];
	if (meta.port !== undefined) bits.push(`port ${meta.port}`);
	if (meta.grantId) bits.push(`grant ${meta.grantId}`);
	else if (meta.tag) bits.push(meta.tag);
	else if (meta.endpointId) bits.push(`endpoint ${shortId(meta.endpointId)}`);

	return (
		<span className="font-mono text-sm opacity-70 truncate block">
			{bits.length > 0 ? bits.join(" - ") : "\u00a0"}
		</span>
	);
}
