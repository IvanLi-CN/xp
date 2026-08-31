import type { ServiceMonitorStatus } from "../api/adminServiceMonitors";
import { Badge } from "./ui/badge";

type ServiceMonitorStatusBadgeProps = {
	status: ServiceMonitorStatus;
	stale?: boolean;
};

const labels: Record<ServiceMonitorStatus, string> = {
	up: "Up",
	degraded: "Degraded",
	down: "Down",
	unknown: "Unknown",
	capture_suspended: "Capture suspended",
};

export function ServiceMonitorStatusBadge({
	status,
	stale = false,
}: ServiceMonitorStatusBadgeProps) {
	if (stale && status !== "capture_suspended") {
		return (
			<Badge variant="warning" size="sm">
				Stale
			</Badge>
		);
	}
	const variant =
		status === "up"
			? "success"
			: status === "degraded" || status === "unknown"
				? "warning"
				: "destructive";
	return (
		<Badge variant={variant} size="sm">
			{labels[status]}
		</Badge>
	);
}
