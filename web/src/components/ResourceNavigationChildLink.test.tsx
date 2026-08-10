import {
	act,
	fireEvent,
	render,
	screen,
	waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";

import { ResourceNavigationChildLink } from "./ResourceNavigationChildLink";

const LONG_LABEL =
	"singapore-edge-with-an-intentionally-long-hostname-for-navigation";
const ARIA_LABEL = `Node ${LONG_LABEL} (node-sgp-1)`;

let resizeCallbacks: ResizeObserverCallback[] = [];

function renderLink(options?: {
	reducedMotion?: boolean;
	label?: string;
	leadingIcon?: "primary" | "muted";
}) {
	const label = options?.label ?? LONG_LABEL;
	return render(
		<TooltipProvider delayDuration={0} skipDelayDuration={0}>
			<ResourceNavigationChildLink
				href="/nodes/node-sgp-1"
				aria-label={options?.label ? `Node ${label}` : ARIA_LABEL}
				label={label}
				isActive
				prefersReducedMotion={options?.reducedMotion ?? false}
				leadingIcon={
					options?.leadingIcon
						? {
								name:
									options.leadingIcon === "primary"
										? "tabler:server-bolt"
										: "tabler:server",
								tone: options.leadingIcon,
							}
						: undefined
				}
			/>
		</TooltipProvider>,
	);
}

function setLabelMeasurements(viewportWidth: number, textWidth: number) {
	const text = screen.getByText(LONG_LABEL);
	const viewport = text.parentElement;
	if (!viewport) throw new Error("Label viewport is missing.");
	Object.defineProperty(viewport, "clientWidth", {
		configurable: true,
		value: viewportWidth,
	});
	Object.defineProperty(text, "scrollWidth", {
		configurable: true,
		value: textWidth,
	});
	act(() => {
		for (const callback of resizeCallbacks) {
			callback([], {} as ResizeObserver);
		}
	});
	return { text, viewport };
}

describe("<ResourceNavigationChildLink />", () => {
	beforeEach(() => {
		resizeCallbacks = [];
		globalThis.ResizeObserver = class {
			constructor(callback: ResizeObserverCallback) {
				resizeCallbacks.push(callback);
			}
			disconnect() {}
			observe() {}
			unobserve() {}
		} as typeof ResizeObserver;
	});

	afterEach(() => {
		vi.useRealTimers();
	});

	it("keeps short labels still and does not expose a native or custom tooltip", () => {
		renderLink({ reducedMotion: true });
		const { text, viewport } = setLabelMeasurements(240, 180);
		const link = screen.getByRole("link", { name: ARIA_LABEL });

		expect(link).not.toHaveAttribute("title");
		expect(viewport).toHaveAttribute("data-overflowing", "false");
		expect(viewport.style.maskImage).toBe("");
		expect(text.style.transform).toBe("translateX(0)");
		fireEvent.focus(link);
		expect(screen.queryByRole("tooltip")).toBeNull();
	});

	it("reveals an overflowing label after hover delay and returns to the start", () => {
		vi.useFakeTimers();
		renderLink({ reducedMotion: false, leadingIcon: "primary" });
		const { text, viewport } = setLabelMeasurements(120, 300);
		const link = screen.getByRole("link", { name: ARIA_LABEL });

		expect(link).toHaveAttribute(
			"data-leading-icon-name",
			"tabler:server-bolt",
		);
		expect(link).toHaveAttribute("data-leading-icon-tone", "primary");
		expect(viewport).toHaveAttribute("data-overflowing", "true");
		expect(viewport).toHaveAttribute("data-reveal-phase", "start");
		expect(viewport.style.maskImage).toContain("calc(100% - 1rem)");

		vi.spyOn(link, "matches").mockReturnValue(false);
		fireEvent.pointerEnter(link);
		fireEvent.focus(link);
		act(() => vi.advanceTimersByTime(349));
		expect(viewport).toHaveAttribute("data-reveal-phase", "start");
		act(() => vi.advanceTimersByTime(1));
		expect(viewport).toHaveAttribute("data-reveal-phase", "forward");
		expect(text.style.transform).toBe("translateX(-180px)");
		expect(screen.queryByRole("tooltip")).toBeNull();

		fireEvent.transitionEnd(text, { propertyName: "transform" });
		expect(viewport).toHaveAttribute("data-reveal-phase", "end");
		fireEvent.pointerLeave(link);
		expect(viewport).toHaveAttribute("data-reveal-phase", "return");
		fireEvent.transitionEnd(text, { propertyName: "transform" });
		expect(viewport).toHaveAttribute("data-reveal-phase", "start");
		expect(text.style.transform).toBe("translateX(0)");
	});

	it("starts immediately on focus when motion is allowed", () => {
		renderLink({ reducedMotion: false });
		const { viewport } = setLabelMeasurements(120, 300);
		const link = screen.getByRole("link", { name: ARIA_LABEL });
		vi.spyOn(link, "matches").mockReturnValue(true);
		fireEvent.focus(link);
		expect(viewport).toHaveAttribute("data-reveal-phase", "forward");
	});

	it("uses only the custom tooltip for reduced motion overflow", async () => {
		renderLink({ reducedMotion: true });
		const { text, viewport } = setLabelMeasurements(120, 300);
		const link = screen.getByRole("link", { name: ARIA_LABEL });

		fireEvent.focus(link);
		expect(viewport).toHaveAttribute("data-reveal-phase", "start");
		expect(text.style.transform).toBe("translateX(0)");
		expect(await screen.findByRole("tooltip")).toHaveTextContent(LONG_LABEL);

		fireEvent.keyDown(link, { key: "Escape" });
		await waitFor(() => expect(screen.queryByRole("tooltip")).toBeNull());
	});
});
