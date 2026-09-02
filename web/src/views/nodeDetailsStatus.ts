import type { NodeRuntimeEvent } from "../api/adminNodeRuntime";

export function summaryBadgeVariant(status: string) {
	switch (status) {
		case "up":
			return "success";
		case "degraded":
			return "warning";
		case "down":
			return "destructive";
		default:
			return "ghost";
	}
}

export function componentBadgeVariant(status: string) {
	switch (status) {
		case "up":
			return "success";
		case "degraded":
			return "warning";
		case "down":
			return "destructive";
		case "disabled":
			return "ghost";
		default:
			return "outline";
	}
}

export function eventBadgeVariant(kind: NodeRuntimeEvent["kind"]) {
	switch (kind) {
		case "status_changed":
			return "warning";
		case "restart_requested":
			return "info";
		case "restart_succeeded":
			return "success";
		case "restart_failed":
			return "destructive";
		default:
			return "ghost";
	}
}

export function historySlotClass(status: string): string {
	switch (status) {
		case "up":
			return "bg-success";
		case "degraded":
			return "bg-warning";
		case "down":
			return "bg-destructive";
		case "unknown":
			return "bg-info";
		default:
			return "bg-muted";
	}
}
