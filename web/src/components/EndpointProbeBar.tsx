import type { EndpointProbeStatus } from "../api/adminEndpoints";
import type { AdminEndpointProbeSlot } from "../api/adminEndpoints";

function statusClass(status: EndpointProbeStatus): string {
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

export function EndpointProbeBar(props: {
	slots: AdminEndpointProbeSlot[];
	className?: string;
}) {
	const { slots, className } = props;

	if (slots.length === 0) {
		return <span className="opacity-60">-</span>;
	}

	return (
		<div
			className={["flex items-end gap-px", className].filter(Boolean).join(" ")}
		>
			{slots.map((slot) => (
				<div
					key={slot.hour}
					title={`${slot.hour} • ${slot.status}`}
					className={[
						"h-4 w-1 rounded-sm",
						statusClass(slot.status),
						slot.status === "missing" ? "opacity-50" : "",
					]
						.filter(Boolean)
						.join(" ")}
				/>
			))}
		</div>
	);
}
