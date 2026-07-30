import { useLayoutEffect, useMemo, useState } from "react";

import { useUiPrefsOptional } from "./UiPrefs";

const FALLBACKS = {
	axis: "rgba(148, 163, 184, 0.78)",
	currentArea: "rgba(6, 182, 212, 0.18)",
	currentDay: "rgba(6, 182, 212, 0.07)",
	grid: "rgba(148, 163, 184, 0.18)",
	primary: "rgb(6, 182, 212)",
	reference: "rgba(148, 163, 184, 0.82)",
	tooltipBackground: "rgb(34, 40, 52)",
	tooltipBorder: "rgba(148, 163, 184, 0.24)",
	tooltipForeground: "rgb(226, 232, 240)",
	tooltipLightShadow: "rgba(15, 23, 42, 0.18)",
	tooltipMuted: "rgb(148, 163, 184)",
	tooltipShadow: "rgba(15, 23, 42, 0.28)",
} as const;

export type EChartsTooltipPalette = {
	background: string;
	border: string;
	foreground: string;
	muted: string;
	shadow: string;
	shadowBlur: number;
	shadowOffsetX: number;
	shadowOffsetY: number;
};

export type EChartsThemePalette = {
	axis: string;
	axisPointer: string;
	currentArea: string;
	currentDay: string;
	grid: string;
	primary: string;
	reference: string;
	tooltip: EChartsTooltipPalette;
};

export const STATIC_LINE_SERIES_EMPHASIS = { disabled: true } as const;

function rgbaFromCanvasColor(value: string, fallback: string): string {
	if (typeof document === "undefined" || !document.body) return fallback;
	const probe = document.createElement("span");
	probe.style.color = value;
	if (!probe.style.color) return fallback;
	probe.style.opacity = "0";
	probe.style.pointerEvents = "none";
	probe.style.position = "fixed";
	document.body.append(probe);
	const color = getComputedStyle(probe).color;
	probe.remove();
	if (
		typeof navigator !== "undefined" &&
		navigator.userAgent.toLowerCase().includes("jsdom")
	) {
		return color || fallback;
	}

	const canvas = document.createElement("canvas");
	canvas.width = 1;
	canvas.height = 1;
	const context = canvas.getContext("2d", { willReadFrequently: true });
	if (!context) return color || fallback;
	context.clearRect(0, 0, 1, 1);
	context.fillStyle = color;
	context.fillRect(0, 0, 1, 1);
	const [red, green, blue, alpha] = context.getImageData(0, 0, 1, 1).data;
	return `rgba(${red}, ${green}, ${blue}, ${(alpha / 255).toFixed(3)})`;
}

export function resolveEChartsCssColor(
	value: string,
	fallback: string,
): string {
	return rgbaFromCanvasColor(value, fallback);
}

function resolvePalette(resolvedTheme?: "light" | "dark"): EChartsThemePalette {
	const isLightTheme = resolvedTheme !== "dark";
	return {
		axis: resolveEChartsCssColor(
			"color-mix(in srgb, var(--color-muted-foreground) 78%, transparent)",
			FALLBACKS.axis,
		),
		axisPointer: resolveEChartsCssColor(
			"color-mix(in srgb, var(--color-muted-foreground) 72%, transparent)",
			FALLBACKS.axis,
		),
		currentArea: resolveEChartsCssColor(
			"color-mix(in srgb, var(--color-primary) 18%, transparent)",
			FALLBACKS.currentArea,
		),
		currentDay: resolveEChartsCssColor(
			"color-mix(in srgb, var(--color-primary) 7%, transparent)",
			FALLBACKS.currentDay,
		),
		grid: resolveEChartsCssColor(
			"color-mix(in srgb, var(--color-muted-foreground) 18%, transparent)",
			FALLBACKS.grid,
		),
		primary: resolveEChartsCssColor("var(--color-primary)", FALLBACKS.primary),
		reference: resolveEChartsCssColor(
			"color-mix(in srgb, var(--color-muted-foreground) 82%, transparent)",
			FALLBACKS.reference,
		),
		tooltip: {
			background: resolveEChartsCssColor(
				"var(--popover)",
				FALLBACKS.tooltipBackground,
			),
			border: resolveEChartsCssColor("var(--border)", FALLBACKS.tooltipBorder),
			foreground: resolveEChartsCssColor(
				"var(--popover-foreground)",
				FALLBACKS.tooltipForeground,
			),
			muted: resolveEChartsCssColor(
				"var(--muted-foreground)",
				FALLBACKS.tooltipMuted,
			),
			shadow: resolveEChartsCssColor(
				isLightTheme
					? "color-mix(in srgb, var(--foreground) 18%, transparent)"
					: "var(--xp-overlay)",
				isLightTheme ? FALLBACKS.tooltipLightShadow : FALLBACKS.tooltipShadow,
			),
			shadowBlur: isLightTheme ? 12 : 18,
			shadowOffsetX: 0,
			shadowOffsetY: isLightTheme ? 4 : 10,
		},
	};
}

export function useEChartsThemePalette(): EChartsThemePalette {
	const prefs = useUiPrefsOptional();
	const resolvedTheme = prefs?.resolvedTheme;
	const [paletteState, setPaletteState] = useState(() => ({
		theme: resolvedTheme,
		revision: 0,
	}));

	// UiPrefs applies the root theme in a layout effect. Refresh before paint so the
	// palette sees the committed CSS tokens after an initial preference restore.
	useLayoutEffect(() => {
		setPaletteState((state) => ({
			theme: resolvedTheme,
			revision: state.revision + 1,
		}));
	}, [resolvedTheme]);

	return useMemo(() => resolvePalette(paletteState.theme), [paletteState]);
}

export function createThemedAxisPointer(palette: EChartsThemePalette) {
	return {
		type: "line" as const,
		lineStyle: {
			color: palette.axisPointer,
			type: "dashed" as const,
			width: 1,
		},
	};
}

export function createThemedTooltipSurface(palette: EChartsThemePalette) {
	return {
		axisPointer: createThemedAxisPointer(palette),
		backgroundColor: palette.tooltip.background,
		borderColor: palette.tooltip.border,
		borderWidth: 1,
		extraCssText:
			"border-radius: 0.75rem;max-width:min(22rem, calc(100vw - 4rem));white-space:normal;",
		padding: 12,
		shadowBlur: palette.tooltip.shadowBlur,
		shadowColor: palette.tooltip.shadow,
		shadowOffsetX: palette.tooltip.shadowOffsetX,
		shadowOffsetY: palette.tooltip.shadowOffsetY,
		textStyle: { color: palette.tooltip.foreground },
	};
}

export function escapeEChartsHtml(value: string): string {
	return value
		.replaceAll("&", "&amp;")
		.replaceAll("<", "&lt;")
		.replaceAll(">", "&gt;")
		.replaceAll('"', "&quot;")
		.replaceAll("'", "&#39;");
}
