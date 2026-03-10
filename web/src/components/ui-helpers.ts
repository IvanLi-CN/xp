import { cn } from "@/lib/utils";
import type { UiDensity } from "./UiPrefs";

export function densityControlClass(base: string, density: UiDensity) {
	return cn(base, density === "compact" && `${base}-sm`);
}

export function inputClass(
	density: UiDensity,
	...extra: Array<string | false | null | undefined>
) {
	return cn(densityControlClass("xp-input", density), ...extra);
}

export function selectClass(
	density: UiDensity,
	...extra: Array<string | false | null | undefined>
) {
	return cn(densityControlClass("xp-select", density), ...extra);
}

export function textareaClass(
	...extra: Array<string | false | null | undefined>
) {
	return cn("xp-textarea", ...extra);
}

export function badgeClass(
	tone:
		| "ghost"
		| "outline"
		| "primary"
		| "success"
		| "warning"
		| "error"
		| "info"
		| "neutral",
	size: "default" | "sm" = "default",
	...extra: Array<string | false | null | undefined>
) {
	return cn(
		"xp-badge",
		size === "sm" && "xp-badge-sm",
		`xp-badge-${tone}`,
		...extra,
	);
}

export function alertClass(
	tone: "info" | "warning" | "error",
	...extra: Array<string | false | null | undefined>
) {
	return cn("xp-alert", `xp-alert-${tone}`, ...extra);
}

export function tableClass(
	compact: boolean,
	...extra: Array<string | false | null | undefined>
) {
	return cn("xp-table xp-table-zebra", compact && "xp-table-compact", ...extra);
}
