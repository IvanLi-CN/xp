import { useCallback, useEffect, useRef, useState } from "react";

import type { NodeRuntimeHistorySlot } from "../api/adminNodeRuntime";
import { IconButton } from "../components/Button";
import { Icon } from "../components/Icon";
import { historySlotClass } from "./nodeDetailsStatus";

const SLOTS_PER_DAY = 48;
const ACTIVITY_DAYS = 7;

type RuntimeActivityRow = {
	key: string;
	label: string;
	sortKey: number;
	slots: Array<NodeRuntimeHistorySlot | null>;
};

function buildRuntimeActivityRows(
	recentSlots: NodeRuntimeHistorySlot[],
): RuntimeActivityRow[] {
	const byDay = new Map<string, RuntimeActivityRow>();
	for (const slot of recentSlots) {
		const at = new Date(slot.slot_start);
		if (Number.isNaN(at.getTime())) continue;
		const dayStart = new Date(at.getFullYear(), at.getMonth(), at.getDate());
		const month = String(dayStart.getMonth() + 1).padStart(2, "0");
		const day = String(dayStart.getDate()).padStart(2, "0");
		const key = `${dayStart.getFullYear()}-${month}-${day}`;

		let row = byDay.get(key);
		if (!row) {
			row = {
				key,
				sortKey: dayStart.getTime(),
				label: dayStart.toLocaleDateString(undefined, {
					month: "numeric",
					day: "numeric",
					weekday: "short",
				}),
				slots: new Array(SLOTS_PER_DAY).fill(null),
			};
			byDay.set(key, row);
		}

		const slotIndex = at.getHours() * 2 + (at.getMinutes() >= 30 ? 1 : 0);
		if (slotIndex >= 0 && slotIndex < SLOTS_PER_DAY) {
			row.slots[slotIndex] = slot;
		}
	}

	const rows = Array.from(byDay.values()).sort((a, b) => a.sortKey - b.sortKey);
	if (rows.length > 0) {
		return rows.slice(-ACTIVITY_DAYS);
	}

	const fallbackRows: RuntimeActivityRow[] = [];
	const fallbackSlots = recentSlots.slice(-SLOTS_PER_DAY * ACTIVITY_DAYS);
	for (let dayIndex = 0; dayIndex < ACTIVITY_DAYS; dayIndex += 1) {
		const sliceStart = dayIndex * SLOTS_PER_DAY;
		const daySlots = fallbackSlots.slice(
			sliceStart,
			sliceStart + SLOTS_PER_DAY,
		);
		if (daySlots.length === 0) continue;
		fallbackRows.push({
			key: `fallback-${dayIndex}`,
			label: `day ${dayIndex + 1}`,
			sortKey: dayIndex,
			slots: [
				...daySlots,
				...new Array(Math.max(0, SLOTS_PER_DAY - daySlots.length)).fill(null),
			],
		});
	}

	return fallbackRows;
}

export function NodeRuntimeActivityChart({
	recentSlots,
}: {
	recentSlots: NodeRuntimeHistorySlot[];
}) {
	const scrollRegionRef = useRef<HTMLDivElement>(null);
	const [scrollState, setScrollState] = useState({
		canScrollLeft: false,
		canScrollRight: false,
	});

	const updateScrollState = useCallback(() => {
		const scrollRegion = scrollRegionRef.current;
		if (!scrollRegion) return;

		const maxScrollLeft = Math.max(
			0,
			scrollRegion.scrollWidth - scrollRegion.clientWidth,
		);
		setScrollState((current) => {
			const next = {
				canScrollLeft: scrollRegion.scrollLeft > 0,
				canScrollRight: scrollRegion.scrollLeft < maxScrollLeft - 1,
			};
			return current.canScrollLeft === next.canScrollLeft &&
				current.canScrollRight === next.canScrollRight
				? current
				: next;
		});
	}, []);

	useEffect(() => {
		const scrollRegion = scrollRegionRef.current;
		if (!scrollRegion) return;

		updateScrollState();
		scrollRegion.addEventListener("scroll", updateScrollState, {
			passive: true,
		});
		window.addEventListener("resize", updateScrollState);
		return () => {
			scrollRegion.removeEventListener("scroll", updateScrollState);
			window.removeEventListener("resize", updateScrollState);
		};
	}, [updateScrollState]);

	const scrollActivityChart = (direction: -1 | 1) => {
		const scrollRegion = scrollRegionRef.current;
		if (!scrollRegion) return;

		const maxScrollLeft = Math.max(
			0,
			scrollRegion.scrollWidth - scrollRegion.clientWidth,
		);
		const scrollDistance = Math.max(scrollRegion.clientWidth * 0.75, 160);
		scrollRegion.scrollLeft = Math.min(
			maxScrollLeft,
			Math.max(0, scrollRegion.scrollLeft + direction * scrollDistance),
		);
		updateScrollState();
	};

	return (
		<div className="min-w-0 rounded-2xl border border-border/70 bg-muted/35 p-3">
			<div className="mb-2 flex justify-end gap-1 sm:hidden">
				<IconButton
					variant="ghost"
					label="Scroll activity chart left"
					tooltip="Scroll activity chart left"
					disabled={!scrollState.canScrollLeft}
					onClick={() => scrollActivityChart(-1)}
				>
					<Icon name="tabler:chevron-left" size={16} />
				</IconButton>
				<IconButton
					variant="ghost"
					label="Scroll activity chart right"
					tooltip="Scroll activity chart right"
					disabled={!scrollState.canScrollRight}
					onClick={() => scrollActivityChart(1)}
				>
					<Icon name="tabler:chevron-right" size={16} />
				</IconButton>
			</div>
			<section
				ref={scrollRegionRef}
				aria-label="7-day service activity chart"
				data-testid="node-runtime-activity-chart-scroll"
				className="max-w-full overflow-x-auto overscroll-x-contain rounded-[inherit]"
			>
				<div className="min-w-[28rem]">
					<div
						className="mb-1 grid items-center gap-2 text-xs text-muted-foreground"
						style={{
							gridTemplateColumns: "4.5rem minmax(0,1fr)",
						}}
					>
						<span className="sticky left-0 z-10 bg-card" />
						<div className="flex items-center justify-between">
							<span>00:00</span>
							<span>06:00</span>
							<span>12:00</span>
							<span>18:00</span>
							<span>24:00</span>
						</div>
					</div>
					<div className="space-y-1.5">
						{buildRuntimeActivityRows(recentSlots).map((row) => (
							<div
								key={row.key}
								className="grid items-center gap-2"
								style={{
									gridTemplateColumns: "4.5rem minmax(0,1fr)",
								}}
							>
								<span
									className={[
										"sticky left-0 z-10 truncate bg-card pr-2",
										"font-mono text-xs text-muted-foreground",
									].join(" ")}
								>
									{row.label}
								</span>
								<div
									className="grid h-3 min-w-0 gap-px"
									style={{
										gridTemplateColumns: "repeat(48, minmax(0, 1fr))",
									}}
								>
									{row.slots.map((slot, index) => (
										<div
											key={`${row.key}-${index}`}
											className={`rounded-[1px] ${
												slot ? historySlotClass(slot.status) : "bg-muted/60"
											}`}
											title={
												slot ? `${slot.slot_start} • ${slot.status}` : undefined
											}
										/>
									))}
								</div>
							</div>
						))}
					</div>
				</div>
			</section>
		</div>
	);
}
