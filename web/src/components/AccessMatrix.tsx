import { type ReactNode, useEffect, useMemo, useRef } from "react";

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
	onSelectCellEndpoint?: (
		nodeId: string,
		protocolId: string,
		endpointId: string,
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

	return (
		<div className="overflow-x-auto">
			<table className="table table-zebra table-sm w-full">
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
								<th key={protocol.protocolId} className="min-w-[180px]">
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
											<div className="flex items-center justify-between gap-2">
												<div className="min-w-0 flex-1">
													<div className="flex items-center gap-2">
														<input
															type="checkbox"
															className="checkbox checkbox-xs checkbox-primary rounded"
															checked={cell.value === "on"}
															disabled={disabled || cell.value === "disabled"}
															aria-label={`Toggle ${node.label} ${protocol.label}`}
															onChange={() =>
																onToggleCell?.(node.nodeId, protocol.protocolId)
															}
														/>
														<div className="min-w-0 flex-1">
															{cell.value === "disabled" ? (
																<span className="text-xs opacity-60">
																	{cell.reason ?? "Disabled"}
																</span>
															) : (
																<AccessMatrixCellLabel
																	nodeId={node.nodeId}
																	protocolId={protocol.protocolId}
																	cell={cell}
																	disabled={disabled}
																	onSelectCellEndpoint={onSelectCellEndpoint}
																/>
															)}
														</div>
													</div>
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

function shortId(id: string): string {
	return id.length <= 8 ? id : `${id.slice(0, 4)}..${id.slice(-4)}`;
}

function AccessMatrixCellLabel(props: {
	nodeId: string;
	protocolId: string;
	cell: AccessMatrixCellState;
	disabled?: boolean;
	onSelectCellEndpoint?: (
		nodeId: string,
		protocolId: string,
		endpointId: string,
	) => void;
}) {
	const { nodeId, protocolId, cell, disabled, onSelectCellEndpoint } = props;
	const meta = cell.meta;
	if (!meta) return null;

	const options = meta.options ?? [];
	if (options.length > 1) {
		return (
			<div className="flex items-center justify-between gap-2">
				<div className="min-w-0 flex-1">
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
					<button
						type="button"
						className="btn btn-ghost btn-xs"
						disabled={disabled}
					>
						Choose ({options.length})
					</button>
					<ul className="dropdown-content menu z-[1] w-64 rounded-box border border-base-200 bg-base-100 shadow">
						{options.map((opt) => (
							<li key={opt.endpointId}>
								<button
									type="button"
									className="justify-between"
									disabled={disabled}
									onClick={() => {
										if (disabled) return;
										onSelectCellEndpoint?.(nodeId, protocolId, opt.endpointId);
									}}
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
	else if (meta.tag) bits.push(meta.tag);
	else if (meta.endpointId) bits.push(`endpoint ${shortId(meta.endpointId)}`);

	return (
		<span className="font-mono text-sm opacity-70 truncate block">
			{bits.length > 0 ? bits.join(" - ") : "\u00a0"}
		</span>
	);
}
