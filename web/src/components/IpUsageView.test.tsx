import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { AdminNodeIpUsageResponse } from "../api/adminIpUsage";
import { IpUsageView, SUMMARY_HIGHLIGHT_BADGE_STYLE } from "./IpUsageView";

const stylesCss = readFileSync(
	resolve(process.cwd(), "src/styles.css"),
	"utf8",
);
const rootThemeBlock = stylesCss.match(/:root\s*\{([\s\S]*?)\n\}/)?.[1];
const darkThemeBlock = stylesCss.match(/\.dark\s*\{([\s\S]*?)\n\}/)?.[1];

if (!rootThemeBlock || !darkThemeBlock) {
	throw new Error("Failed to load theme tokens from styles.css");
}

const summaryPanelAlpha = 0.3;
const minBadgeContrast = 4.5;

type ThemeName = "light" | "dark";
type SummaryHighlightTone = keyof typeof SUMMARY_HIGHLIGHT_BADGE_STYLE;
type ThemeToken =
	| "card"
	| "muted"
	| "foreground"
	| "info"
	| "info-foreground"
	| "warning"
	| "warning-foreground";
type OklchColor = [number, number, number];
type SrgbColor = [number, number, number];

const themeBlocks: Record<ThemeName, string> = {
	light: rootThemeBlock,
	dark: darkThemeBlock,
};

function themeToken(theme: ThemeName, token: ThemeToken): OklchColor {
	const tokenMatch = themeBlocks[theme].match(
		new RegExp(`--${token}:\\s*oklch\\(([^)]+)\\);`),
	)?.[1];
	if (!tokenMatch) {
		throw new Error(`Missing ${token} token for ${theme} theme`);
	}
	const [lightness, chroma, hue] = tokenMatch.trim().split(/\s+/);
	return [
		Number.parseFloat(lightness) / 100,
		Number.parseFloat(chroma),
		Number.parseFloat(hue),
	];
}

function oklchToSrgb([lightness, chroma, hue]: OklchColor): SrgbColor {
	const hueRadians = (hue * Math.PI) / 180;
	const a = chroma * Math.cos(hueRadians);
	const b = chroma * Math.sin(hueRadians);
	const lPrime = lightness + 0.3963377774 * a + 0.2158037573 * b;
	const mPrime = lightness - 0.1055613458 * a - 0.0638541728 * b;
	const sPrime = lightness - 0.0894841775 * a - 1.291485548 * b;
	const l = lPrime ** 3;
	const m = mPrime ** 3;
	const s = sPrime ** 3;
	const linear = [
		4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
		-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
		-0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
	] as const;
	return linear.map((channel) => {
		const clamped = Math.min(1, Math.max(0, channel));
		if (clamped <= 0.0031308) {
			return 12.92 * clamped;
		}
		return 1.055 * clamped ** (1 / 2.4) - 0.055;
	}) as SrgbColor;
}

function blendSrgb(
	foreground: SrgbColor,
	alpha: number,
	background: SrgbColor,
): SrgbColor {
	return foreground.map(
		(channel, index) => alpha * channel + (1 - alpha) * background[index],
	) as SrgbColor;
}

function srgbToLinear(channel: number): number {
	if (channel <= 0.04045) {
		return channel / 12.92;
	}
	return ((channel + 0.055) / 1.055) ** 2.4;
}

function contrastRatio(foreground: SrgbColor, background: SrgbColor): number {
	const luminance = (color: SrgbColor) => {
		const [red, green, blue] = color.map(srgbToLinear);
		return 0.2126 * red + 0.7152 * green + 0.0722 * blue;
	};
	const foregroundLuminance = luminance(foreground);
	const backgroundLuminance = luminance(background);
	const lighter = Math.max(foregroundLuminance, backgroundLuminance);
	const darker = Math.min(foregroundLuminance, backgroundLuminance);
	return (lighter + 0.05) / (darker + 0.05);
}

function summaryBadgeContrast(
	theme: ThemeName,
	tone: SummaryHighlightTone,
	target: "label" | "value",
): number {
	const contract = SUMMARY_HIGHLIGHT_BADGE_STYLE[tone].contrast;
	const cardBackground = oklchToSrgb(themeToken(theme, "card"));
	const summaryBackground = blendSrgb(
		oklchToSrgb(themeToken(theme, "muted")),
		summaryPanelAlpha,
		cardBackground,
	);
	const badgeBackground = blendSrgb(
		oklchToSrgb(themeToken(theme, tone)),
		theme === "light"
			? contract.lightBackgroundAlpha
			: contract.darkBackgroundAlpha,
		summaryBackground,
	);
	const textToken =
		theme === "light" ? contract.lightTextToken : contract.darkTextToken;
	const textColor = oklchToSrgb(themeToken(theme, textToken));
	const foreground =
		target === "label"
			? blendSrgb(textColor, contract.labelOpacity, badgeBackground)
			: textColor;
	return contrastRatio(foreground, badgeBackground);
}

const baseReport: Pick<
	AdminNodeIpUsageResponse,
	| "window_start"
	| "window_end"
	| "warnings"
	| "unique_ip_series"
	| "timeline"
	| "ips"
> = {
	window_start: "2026-03-08T00:00:00Z",
	window_end: "2026-03-08T00:02:00Z",
	warnings: [],
	unique_ip_series: [
		{ minute: "2026-03-08T00:00:00Z", count: 1 },
		{ minute: "2026-03-08T00:01:00Z", count: 2 },
		{ minute: "2026-03-08T00:02:00Z", count: 1 },
	],
	timeline: [
		{
			lane_key: "edge-tokyo|203.0.113.7",
			endpoint_id: "endpoint-1",
			endpoint_tag: "edge-tokyo",
			ip: "203.0.113.7",
			minutes: 2,
			segments: [
				{
					start_minute: "2026-03-08T00:00:00Z",
					end_minute: "2026-03-08T00:01:00Z",
				},
			],
		},
		{
			lane_key: "edge-osaka|198.51.100.4",
			endpoint_id: "endpoint-2",
			endpoint_tag: "edge-osaka",
			ip: "198.51.100.4",
			minutes: 1,
			segments: [
				{
					start_minute: "2026-03-08T00:02:00Z",
					end_minute: "2026-03-08T00:02:00Z",
				},
			],
		},
	],
	ips: [
		{
			ip: "203.0.113.7",
			minutes: 2,
			endpoint_tags: ["edge-tokyo"],
			region: "Japan / Tokyo",
			operator: "ExampleNet",
			last_seen_at: "2026-03-08T00:01:00Z",
		},
		{
			ip: "198.51.100.4",
			minutes: 1,
			endpoint_tags: ["edge-osaka"],
			region: "Singapore",
			operator: "LionLink",
			last_seen_at: "2026-03-08T00:02:00Z",
		},
	],
};

describe("<IpUsageView />", () => {
	it("switches between 24h and 7d windows", () => {
		const onWindowChange = vi.fn();
		render(
			<IpUsageView
				title="IP usage"
				description="Node inbound IP snapshots"
				window="24h"
				onWindowChange={onWindowChange}
				report={baseReport}
			/>,
		);

		fireEvent.click(screen.getByRole("button", { name: "7d" }));
		expect(onWindowChange).toHaveBeenCalledWith("7d");
		expect(
			screen.getByText("Unique IPs per minute", { selector: "p" }),
		).toBeInTheDocument();
		expect(screen.getByText("IP occupancy lanes")).toBeInTheDocument();
	});

	it("highlights matching rows when hovering and clicking IP/time controls", () => {
		render(
			<IpUsageView
				title="IP usage"
				description="Node inbound IP snapshots"
				window="24h"
				onWindowChange={vi.fn()}
				report={baseReport}
			/>,
		);

		const ipButton = screen.getAllByRole("button", { name: "203.0.113.7" })[0];
		const tokyoRow = ipButton?.closest("tr");
		const osakaRow = screen
			.getAllByRole("button", { name: "198.51.100.4" })[0]
			?.closest("tr");
		expect(tokyoRow).not.toBeNull();
		expect(osakaRow).not.toBeNull();

		fireEvent.mouseEnter(ipButton);
		expect(screen.getAllByText("203.0.113.7").length).toBeGreaterThan(1);
		expect(tokyoRow).toHaveClass("bg-info/8");
		expect(osakaRow).toHaveClass("opacity-45");

		fireEvent.click(ipButton);
		expect(ipButton).toHaveAttribute("aria-pressed", "true");
		expect(
			screen.getByRole("button", { name: "Clear pinned highlight" }),
		).toBeInTheDocument();
		const ipBadge = screen
			.getAllByText("203.0.113.7")
			.map((node) => node.closest(".xp-badge"))
			.find((node): node is HTMLElement => node !== null);
		expect(ipBadge).toBeTruthy();
		expect(ipBadge?.className).toContain("bg-info/12");
		expect(ipBadge?.className).toContain("text-foreground");
		expect(ipBadge?.className).toContain("dark:bg-info/80");
		expect(ipBadge?.className).toContain("dark:text-info-foreground");
		expect(within(ipBadge as HTMLElement).getByText("IP")).toHaveClass(
			"opacity-90",
		);

		const lastSeenButton = within(tokyoRow as HTMLTableRowElement).getAllByRole(
			"button",
		)[1];
		fireEvent.click(lastSeenButton);
		expect(lastSeenButton).toHaveAttribute("aria-pressed", "true");
		const timeBadge = screen.getByText("Time").closest(".xp-badge");
		expect(timeBadge).not.toBeNull();
		expect(timeBadge?.className).toContain("bg-warning/12");
		expect(timeBadge?.className).toContain("dark:bg-warning/70");
		expect(timeBadge?.className).toContain("dark:text-warning-foreground");
		expect(within(timeBadge as HTMLElement).getByText("Time")).toHaveClass(
			"opacity-90",
		);
	});

	it("keeps summary highlight badges above AA contrast in light and dark themes", () => {
		for (const theme of ["light", "dark"] as const) {
			for (const tone of Object.keys(
				SUMMARY_HIGHLIGHT_BADGE_STYLE,
			) as SummaryHighlightTone[]) {
				expect(
					summaryBadgeContrast(theme, tone, "value"),
				).toBeGreaterThanOrEqual(minBadgeContrast);
				expect(
					summaryBadgeContrast(theme, tone, "label"),
				).toBeGreaterThanOrEqual(minBadgeContrast);
			}
		}
	});

	it("shows country.is attribution", () => {
		render(
			<IpUsageView
				title="IP usage"
				description="Node inbound IP snapshots"
				window="24h"
				geoSource="country_is"
				onWindowChange={vi.fn()}
				report={baseReport}
			/>,
		);

		expect(
			screen.getByText("Geo enrichment uses the free country.is hosted API."),
		).toBeInTheDocument();
	});

	it("shows country.is notice", () => {
		render(
			<IpUsageView
				title="IP usage"
				description="Node inbound IP snapshots"
				window="24h"
				geoSource="country_is"
				onWindowChange={vi.fn()}
				report={baseReport}
			/>,
		);

		expect(
			screen.getByText("Geo enrichment uses the free country.is hosted API."),
		).toBeInTheDocument();
	});

	it("shows blocking online-stats empty state", () => {
		render(
			<IpUsageView
				title="IP usage"
				description="Node inbound IP snapshots"
				window="24h"
				onWindowChange={vi.fn()}
				report={{
					...baseReport,
					warnings: [
						{
							code: "online_stats_unavailable",
							message: "statsUserOnline is unavailable.",
						},
					],
					unique_ip_series: [],
					timeline: [],
					ips: [],
				}}
			/>,
		);

		expect(
			screen.getByText("Online snapshots are unavailable"),
		).toBeInTheDocument();
		expect(
			screen.getByText("statsUserOnline is unavailable."),
		).toBeInTheDocument();
	});
});
