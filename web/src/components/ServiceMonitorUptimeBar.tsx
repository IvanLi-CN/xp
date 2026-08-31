import type { ServiceMonitorStatus } from "../api/adminServiceMonitors";
import { cn } from "../lib/utils";

type ServiceMonitorUptimeBarProps = {
	slots: readonly ServiceMonitorStatus[];
	label: string;
	className?: string;
	prominent?: boolean;
};

const statusClass: Record<ServiceMonitorStatus, string> = {
	up: "bg-success",
	degraded: "bg-warning",
	down: "bg-destructive",
	unknown: "bg-muted-foreground/20",
	capture_suspended: "bg-destructive/55",
};

const statusLabel: Record<ServiceMonitorStatus, string> = {
	up: "up",
	degraded: "degraded",
	down: "down",
	unknown: "missing",
	capture_suspended: "capture suspended",
};

export function ServiceMonitorUptimeBar({
	slots,
	label,
	className,
	prominent = false,
}: ServiceMonitorUptimeBarProps) {
	const counts: Record<ServiceMonitorStatus, number> = {
		up: 0,
		degraded: 0,
		down: 0,
		unknown: 0,
		capture_suspended: 0,
	};
	for (const status of slots) counts[status] += 1;
	const summary = (
		Object.entries(counts) as Array<[ServiceMonitorStatus, number]>
	)
		.filter(([, count]) => count > 0)
		.map(([status, count]) => `${count} ${statusLabel[status]}`)
		.join(", ");

	return (
		<div
			aria-label={`${label}: ${summary || "no observations"}`}
			className={cn(
				"flex min-w-0 items-stretch",
				prominent ? "h-6 gap-px" : "h-5 gap-0.5",
				className,
			)}
		>
			{slots.map((status, index) => {
				const position = `slot-${index}`;
				return (
					<span
						key={position}
						aria-hidden="true"
						className={cn(
							"min-w-[3px] flex-1 rounded-[2px]",
							statusClass[status],
						)}
					/>
				);
			})}
		</div>
	);
}
