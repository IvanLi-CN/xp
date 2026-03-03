import { Link } from "@tanstack/react-router";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";

import type {
	AdminNodeRuntimeListItem,
	NodeRuntimeComponent,
} from "../api/adminNodeRuntime";
import { Button } from "./Button";
import { Icon } from "./Icon";

const BADGE_GAP_PX = 4;

type ProblematicComponent = Pick<NodeRuntimeComponent, "component" | "status">;

export type NodeInventoryListProps = {
	items: AdminNodeRuntimeListItem[];
	partial: boolean;
	unreachableNodes: string[];
	isRefreshing?: boolean;
	onRefresh?: () => void;
};

function componentBadgeClass(status: string): string {
	switch (status) {
		case "up":
			return "badge badge-success badge-sm";
		case "down":
			return "badge badge-error badge-sm";
		case "unknown":
			return "badge badge-warning badge-sm";
		case "disabled":
			return "badge badge-ghost badge-sm";
		default:
			return "badge badge-outline badge-sm";
	}
}

function historySlotClass(status: string): string {
	switch (status) {
		case "up":
			return "bg-success";
		case "degraded":
			return "bg-warning";
		case "down":
			return "bg-error";
		default:
			return "bg-base-300";
	}
}

function overflowBadgeClass(problematic: ProblematicComponent[]): string {
	return problematic.some((item) => item.status === "down")
		? "badge badge-error badge-sm"
		: "badge badge-warning badge-sm";
}

function ProblematicComponentsField({
	problematic,
}: {
	problematic: ProblematicComponent[];
}) {
	const containerRef = useRef<HTMLDivElement | null>(null);
	const componentBadgeRefs = useRef<Array<HTMLSpanElement | null>>([]);
	const plusBadgeRefs = useRef<Record<number, HTMLSpanElement | null>>({});
	const [visibleCount, setVisibleCount] = useState(problematic.length);

	useEffect(() => {
		setVisibleCount(problematic.length);
	}, [problematic.length]);

	useLayoutEffect(() => {
		if (problematic.length <= 1) return;

		let frame = 0;
		const measure = () => {
			const container = containerRef.current;
			if (!container) return;

			const availableWidth = Math.floor(container.clientWidth);
			if (availableWidth <= 0) return;

			const componentWidths = problematic.map((_, index) =>
				Math.ceil(
					componentBadgeRefs.current[index]?.getBoundingClientRect().width ?? 0,
				),
			);
			if (componentWidths.some((width) => width <= 0)) {
				frame = window.requestAnimationFrame(measure);
				return;
			}

			const prefixWidths = new Array(problematic.length + 1).fill(0);
			for (let i = 0; i < problematic.length; i += 1) {
				prefixWidths[i + 1] = prefixWidths[i] + componentWidths[i];
			}

			const allVisibleWidth =
				prefixWidths[problematic.length] +
				BADGE_GAP_PX * Math.max(0, problematic.length - 1);

			let bestVisibleCount = 0;
			if (allVisibleWidth <= availableWidth) {
				bestVisibleCount = problematic.length;
			} else {
				for (let shown = 0; shown <= problematic.length; shown += 1) {
					const remaining = problematic.length - shown;
					const shownWidth =
						prefixWidths[shown] + BADGE_GAP_PX * Math.max(0, shown - 1);

					if (remaining === 0) {
						if (shownWidth <= availableWidth) {
							bestVisibleCount = shown;
						}
						continue;
					}

					const plusWidth = Math.ceil(
						plusBadgeRefs.current[remaining]?.getBoundingClientRect().width ??
							0,
					);
					if (plusWidth <= 0) continue;

					const combinedWidth =
						shownWidth + (shown > 0 ? BADGE_GAP_PX : 0) + plusWidth;
					if (combinedWidth <= availableWidth) {
						bestVisibleCount = shown;
					}
				}
			}

			setVisibleCount((prev) =>
				prev === bestVisibleCount ? prev : bestVisibleCount,
			);
		};

		measure();
		const observer = new ResizeObserver(() => measure());
		if (containerRef.current) observer.observe(containerRef.current);

		return () => {
			if (frame) window.cancelAnimationFrame(frame);
			observer.disconnect();
		};
	}, [problematic]);

	if (problematic.length === 0) {
		return (
			<span
				className="badge badge-success badge-sm"
				title="All monitored components are healthy."
			>
				normal
			</span>
		);
	}

	const shownCount = Math.max(0, Math.min(visibleCount, problematic.length));
	const shown = problematic.slice(0, shownCount);
	const remaining = problematic.slice(shownCount);
	const remainingTitle = remaining
		.map((item) => `${item.component}:${item.status}`)
		.join(", ");

	return (
		<div ref={containerRef} className="max-w-full overflow-hidden">
			<div className="inline-flex items-center gap-1 whitespace-nowrap">
				{shown.map((item, index) => (
					<span
						key={`${item.component}-${item.status}-${index}`}
						className={componentBadgeClass(item.status)}
						title={`${item.component}:${item.status}`}
					>
						{item.component}:{item.status}
					</span>
				))}
				{remaining.length > 0 ? (
					<span
						className={overflowBadgeClass(remaining)}
						title={remainingTitle}
					>
						+{remaining.length}
					</span>
				) : null}
			</div>
			<div
				aria-hidden="true"
				className="pointer-events-none fixed left-[-9999px] top-0 invisible whitespace-nowrap"
			>
				{problematic.map((item, index) => (
					<span
						key={`measure-${item.component}-${index}`}
						ref={(el) => {
							componentBadgeRefs.current[index] = el;
						}}
						className={componentBadgeClass(item.status)}
					>
						{item.component}:{item.status}
					</span>
				))}
				{Array.from({ length: problematic.length }, (_, i) => {
					const count = i + 1;
					return (
						<span
							key={`measure-plus-${count}`}
							ref={(el) => {
								plusBadgeRefs.current[count] = el;
							}}
							className="badge badge-sm"
						>
							+{count}
						</span>
					);
				})}
			</div>
		</div>
	);
}

function NodePanelLink({
	nodeId,
	accessibleNodeLabel,
}: {
	nodeId: string;
	accessibleNodeLabel: string;
}) {
	const openNodePanelLabel = `Open node panel: ${accessibleNodeLabel}`;

	return (
		<Link
			to="/nodes/$nodeId"
			params={{ nodeId }}
			className="inline-flex shrink-0 items-center rounded-sm text-base-content/60 transition-colors hover:text-primary focus-visible:text-primary focus-visible:outline-none"
			title={openNodePanelLabel}
			aria-label={openNodePanelLabel}
		>
			<Icon name="tabler:external-link" size={20} className="h-5 w-5" />
		</Link>
	);
}

function renderHistorySlots(node: AdminNodeRuntimeListItem) {
	return (
		<div
			className="grid h-4 w-full grid-flow-col auto-cols-fr overflow-hidden rounded-sm"
			title="Last 7 days status (30-minute slots)."
		>
			{node.recent_slots.map((slot) => (
				<div
					key={slot.slot_start}
					className={`h-4 ${historySlotClass(slot.status)}`}
					title={`${slot.slot_start} • ${slot.status}`}
				/>
			))}
		</div>
	);
}

export function sortNodeInventoryItems(
	items: AdminNodeRuntimeListItem[],
): AdminNodeRuntimeListItem[] {
	return [...items];
}

export function NodeInventoryList({
	items,
	partial,
	unreachableNodes,
	isRefreshing = false,
	onRefresh,
}: NodeInventoryListProps) {
	const sortedItems = useMemo(() => sortNodeInventoryItems(items), [items]);

	return (
		<div className="space-y-3">
			{partial ? (
				<div className="alert alert-warning">
					<span className="text-sm">
						Partial result: unreachable node(s):{" "}
						<span className="font-mono">
							{unreachableNodes.join(", ") || "-"}
						</span>
					</span>
				</div>
			) : null}
			<div className="flex items-center justify-between gap-3">
				<p className="text-sm opacity-70">
					{sortedItems.length} node{sortedItems.length === 1 ? "" : "s"} total
				</p>
				{onRefresh ? (
					<Button
						variant="secondary"
						loading={isRefreshing}
						onClick={onRefresh}
					>
						Refresh
					</Button>
				) : null}
			</div>
			<div className="rounded-box border border-base-300 bg-base-100 shadow-sm">
				<div className="hidden overflow-x-auto md:block">
					<table className="table table-zebra">
						<thead>
							<tr>
								<th>Name</th>
								<th>Node ID</th>
								<th>API base URL</th>
								<th>Access host</th>
								<th>Components</th>
								<th>7d (30m)</th>
							</tr>
						</thead>
						<tbody>
							{sortedItems.map((node) => {
								const nodeLabel = node.node_name || "(unnamed)";
								const accessibleNodeLabel = node.node_name || node.node_id;

								return (
									<tr key={node.node_id}>
										<td>
											<div className="flex min-w-0 items-center gap-2">
												<span
													className="block min-w-0 truncate"
													title={nodeLabel}
												>
													{nodeLabel}
												</span>
												<NodePanelLink
													nodeId={node.node_id}
													accessibleNodeLabel={accessibleNodeLabel}
												/>
											</div>
										</td>
										<td className="font-mono">{node.node_id}</td>
										<td className="font-mono break-all whitespace-normal">
											{node.api_base_url}
										</td>
										<td className="font-mono break-all whitespace-normal">
											{node.access_host}
										</td>
										<td>
											<div className="max-w-full truncate whitespace-nowrap">
												<ProblematicComponentsField
													problematic={node.components.filter(
														(component) =>
															component.status === "down" ||
															component.status === "unknown",
													)}
												/>
											</div>
										</td>
										<td>{renderHistorySlots(node)}</td>
									</tr>
								);
							})}
						</tbody>
					</table>
				</div>
				<div className="divide-y divide-base-200 md:hidden">
					{sortedItems.map((node) => {
						const nodeLabel = node.node_name || "(unnamed)";
						const accessibleNodeLabel = node.node_name || node.node_id;
						return (
							<div key={node.node_id} className="space-y-3 px-4 py-3">
								<div className="flex min-w-0 items-start justify-between gap-3">
									<div className="min-w-0 space-y-1">
										<p className="truncate text-sm" title={nodeLabel}>
											{nodeLabel}
										</p>
										<p
											className="break-all font-mono text-xs opacity-70"
											title={node.node_id}
										>
											{node.node_id}
										</p>
									</div>
									<NodePanelLink
										nodeId={node.node_id}
										accessibleNodeLabel={accessibleNodeLabel}
									/>
								</div>
								<div className="space-y-1">
									<p className="text-xs uppercase opacity-60">API base URL</p>
									<p className="break-all font-mono text-xs">
										{node.api_base_url}
									</p>
								</div>
								<div className="space-y-1">
									<p className="text-xs uppercase opacity-60">Access host</p>
									<p className="break-all font-mono text-xs">
										{node.access_host}
									</p>
								</div>
								<div className="space-y-1">
									<p className="text-xs uppercase opacity-60">Components</p>
									<ProblematicComponentsField
										problematic={node.components.filter(
											(component) =>
												component.status === "down" ||
												component.status === "unknown",
										)}
									/>
								</div>
								<div className="space-y-1">
									<p className="text-xs uppercase opacity-60">7d (30m)</p>
									{renderHistorySlots(node)}
								</div>
							</div>
						);
					})}
				</div>
			</div>
		</div>
	);
}
