import {
	type CSSProperties,
	type ComponentPropsWithoutRef,
	forwardRef,
	useEffect,
	useRef,
	useState,
} from "react";

import {
	Tooltip,
	TooltipContent,
	TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

import { Icon } from "./Icon";

const OVERFLOW_THRESHOLD_PX = 1;
const HOVER_DELAY_MS = 350;

type RevealPhase = "start" | "forward" | "end" | "return";

export type ResourceNavigationLeadingIcon = {
	name: string;
	tone: "muted" | "primary";
};

type ResourceNavigationChildLinkProps = Omit<
	ComponentPropsWithoutRef<"a">,
	"children" | "title"
> & {
	label: string;
	leadingIcon?: ResourceNavigationLeadingIcon;
	isActive: boolean;
	prefersReducedMotion: boolean;
};

function clamp(value: number, minimum: number, maximum: number) {
	return Math.min(maximum, Math.max(minimum, value));
}

function maskForPhase(phase: RevealPhase): string {
	if (phase === "start") {
		return [
			"linear-gradient(to right",
			"currentColor 0",
			"currentColor calc(100% - 1rem)",
			"transparent 100%)",
		].join(", ");
	}
	if (phase === "end") {
		return [
			"linear-gradient(to right",
			"transparent 0",
			"currentColor 1rem",
			"currentColor 100%)",
		].join(", ");
	}
	return [
		"linear-gradient(to right",
		"transparent 0",
		"currentColor 1rem",
		"currentColor calc(100% - 1rem)",
		"transparent 100%)",
	].join(", ");
}

export const ResourceNavigationChildLink = forwardRef<
	HTMLAnchorElement,
	ResourceNavigationChildLinkProps
>(
	(
		{
			className,
			label,
			leadingIcon,
			isActive,
			prefersReducedMotion,
			onBlur,
			onFocus,
			onPointerEnter,
			onPointerLeave,
			...props
		},
		ref,
	) => {
		const labelViewportRef = useRef<HTMLSpanElement | null>(null);
		const labelTextRef = useRef<HTMLSpanElement | null>(null);
		const hoverTimerRef = useRef<number | null>(null);
		const [overflowDistance, setOverflowDistance] = useState(0);
		const [phase, setPhase] = useState<RevealPhase>("start");
		const [hovered, setHovered] = useState(false);
		const [focused, setFocused] = useState(false);
		const [tooltipOpen, setTooltipOpen] = useState(false);
		const isOverflowing = overflowDistance > OVERFLOW_THRESHOLD_PX;
		const tooltipEnabled = prefersReducedMotion && isOverflowing;

		useEffect(() => {
			const viewport = labelViewportRef.current;
			const text = labelTextRef.current;
			if (!viewport || !text) return;

			const measure = () => {
				if (text.textContent !== label) return;
				const nextDistance = Math.max(
					0,
					Math.ceil(text.scrollWidth - viewport.clientWidth),
				);
				setOverflowDistance((current) =>
					current === nextDistance ? current : nextDistance,
				);
			};

			measure();
			if (typeof ResizeObserver !== "function") return;
			const observer = new ResizeObserver(measure);
			observer.observe(viewport);
			observer.observe(text);
			return () => observer.disconnect();
		}, [label]);

		useEffect(() => {
			if (hoverTimerRef.current !== null) {
				window.clearTimeout(hoverTimerRef.current);
				hoverTimerRef.current = null;
			}

			if (prefersReducedMotion || !isOverflowing) {
				setPhase("start");
				return;
			}
			if (focused) {
				setPhase((current) => (current === "end" ? current : "forward"));
				return;
			}
			if (hovered) {
				hoverTimerRef.current = window.setTimeout(() => {
					setPhase((current) =>
						current === "end" || current === "forward" ? current : "forward",
					);
					hoverTimerRef.current = null;
				}, HOVER_DELAY_MS);
				return () => {
					if (hoverTimerRef.current !== null) {
						window.clearTimeout(hoverTimerRef.current);
						hoverTimerRef.current = null;
					}
				};
			}
			setPhase((current) => (current === "start" ? current : "return"));
		}, [focused, hovered, isOverflowing, prefersReducedMotion]);

		useEffect(() => {
			if (!tooltipEnabled) setTooltipOpen(false);
		}, [tooltipEnabled]);

		const forwardDuration = clamp((overflowDistance / 60) * 1000, 900, 4000);
		const returnDuration = clamp((overflowDistance / 240) * 1000, 200, 600);
		const translated = phase === "forward" || phase === "end";
		const duration = phase === "return" ? returnDuration : forwardDuration;
		const maskImage = isOverflowing ? maskForPhase(phase) : undefined;
		const viewportStyle: CSSProperties = {
			maskImage,
			WebkitMaskImage: maskImage,
		};
		const textStyle: CSSProperties = {
			transform: translated
				? `translateX(-${overflowDistance}px)`
				: "translateX(0)",
			transitionDuration:
				phase === "forward" || phase === "return" ? `${duration}ms` : "0ms",
			transitionProperty: "transform",
			transitionTimingFunction:
				phase === "return" ? "cubic-bezier(0.4, 0, 0.2, 1)" : "linear",
		};

		return (
			<Tooltip
				open={tooltipEnabled ? tooltipOpen : false}
				onOpenChange={(open) => {
					if (tooltipEnabled) setTooltipOpen(open);
				}}
			>
				<TooltipTrigger asChild>
					<a
						ref={ref}
						data-leading-icon-name={leadingIcon?.name}
						data-leading-icon-tone={leadingIcon?.tone}
						className={cn(
							"flex h-8 w-full min-w-0 items-center overflow-hidden rounded-full px-3",
							"text-xs font-medium text-muted-foreground",
							"transition-colors hover:bg-muted hover:text-foreground",
							"focus-visible:outline-none focus-visible:ring-[3px] focus-visible:ring-ring/20",
							isActive && "bg-primary/10 text-foreground",
							className,
						)}
						onBlur={(event) => {
							setFocused(false);
							onBlur?.(event);
						}}
						onFocus={(event) => {
							setFocused(event.currentTarget.matches(":focus-visible"));
							onFocus?.(event);
						}}
						onPointerEnter={(event) => {
							setHovered(true);
							onPointerEnter?.(event);
						}}
						onPointerLeave={(event) => {
							setHovered(false);
							onPointerLeave?.(event);
						}}
						{...props}
					>
						{leadingIcon ? (
							<Icon
								name={leadingIcon.name}
								className={cn(
									"mr-1 size-4 shrink-0",
									leadingIcon.tone === "primary"
										? "text-primary"
										: "text-muted-foreground/80",
								)}
							/>
						) : null}
						<span
							ref={labelViewportRef}
							data-overflowing={isOverflowing ? "true" : "false"}
							data-reveal-phase={phase}
							className="min-w-0 flex-1 overflow-hidden"
							style={viewportStyle}
						>
							<span
								ref={labelTextRef}
								className="block w-max max-w-none whitespace-nowrap"
								style={textStyle}
								onTransitionEnd={(event) => {
									if (event.propertyName !== "transform") return;
									setPhase((current) => {
										if (current === "forward") return "end";
										if (current === "return") return "start";
										return current;
									});
								}}
							>
								{label}
							</span>
						</span>
					</a>
				</TooltipTrigger>
				{tooltipEnabled ? (
					<TooltipContent
						side="right"
						align="center"
						collisionPadding={12}
						className="whitespace-normal break-words"
					>
						{label}
					</TooltipContent>
				) : null}
			</Tooltip>
		);
	},
);
ResourceNavigationChildLink.displayName = "ResourceNavigationChildLink";
