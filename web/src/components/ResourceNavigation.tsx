import type { MouseEvent } from "react";
import {
	useEffect,
	useId,
	useRef,
	useState,
	useSyncExternalStore,
} from "react";

import { ScrollArea } from "@/components/ui/scroll-area";
import { TooltipProvider } from "@/components/ui/tooltip";

import { Icon } from "./Icon";
import {
	ResourceNavigationChildLink,
	type ResourceNavigationLeadingIcon,
} from "./ResourceNavigationChildLink";

export type ResourceNavigationChild = {
	id: string;
	label: string;
	href: string;
	ariaLabel: string;
	leadingIcon?: ResourceNavigationLeadingIcon;
};

export type ResourceNavigationItem = {
	id: string;
	label: string;
	href: string;
	icon: string;
	children?: ResourceNavigationChild[];
	isLoading?: boolean;
	error?: string | null;
	onRetry?: () => void;
};

export type ResourceNavigationGroup = {
	title: string;
	items: ResourceNavigationItem[];
};

type ResourceNavigationProps = {
	ariaLabel: string;
	groups: ResourceNavigationGroup[];
	pathname: string;
	onNavigate: (href: string) => void;
	onResourceNavigate?: (href: string) => void;
	onResourceRequested?: (resourceId: string) => void;
	reducedMotionOverride?: boolean;
};

const REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)";

const primaryLinkClass = [
	"flex min-w-0 flex-1 items-center gap-3 rounded-xl border border-transparent px-3 py-2",
	"text-sm font-medium text-muted-foreground transition-colors",
	"hover:border-border/70 hover:bg-muted/60 hover:text-foreground",
].join(" ");

const toggleButtonClass = [
	"flex size-8 shrink-0 items-center justify-center rounded-lg text-muted-foreground",
	"transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none",
	"focus-visible:ring-[3px] focus-visible:ring-ring/20",
].join(" ");

const retryButtonClass = [
	"flex size-6 shrink-0 items-center justify-center rounded-md",
	"hover:bg-destructive/10 focus-visible:outline-none",
	"focus-visible:ring-[3px] focus-visible:ring-ring/20",
].join(" ");

function isRouteMatch(pathname: string, href: string) {
	return pathname === href || pathname.startsWith(`${href}/`);
}

function defaultExpanded(groups: ResourceNavigationGroup[], pathname: string) {
	return (
		groups
			.flatMap((group) => group.items)
			.find(
				(item) =>
					item.children !== undefined && pathname.startsWith(`${item.href}/`),
			)?.id ?? null
	);
}

function subscribeToReducedMotion(onChange: () => void) {
	if (
		typeof window === "undefined" ||
		typeof window.matchMedia !== "function"
	) {
		return () => undefined;
	}
	const mediaQuery = window.matchMedia(REDUCED_MOTION_QUERY);
	mediaQuery.addEventListener("change", onChange);
	return () => mediaQuery.removeEventListener("change", onChange);
}

function getReducedMotionSnapshot() {
	return (
		typeof window !== "undefined" &&
		typeof window.matchMedia === "function" &&
		window.matchMedia(REDUCED_MOTION_QUERY).matches
	);
}

function usePrefersReducedMotion() {
	return useSyncExternalStore(
		subscribeToReducedMotion,
		getReducedMotionSnapshot,
		() => false,
	);
}

export function ResourceNavigation({
	ariaLabel,
	groups,
	pathname,
	onNavigate,
	onResourceNavigate,
	onResourceRequested,
	reducedMotionOverride,
}: ResourceNavigationProps) {
	const systemPrefersReducedMotion = usePrefersReducedMotion();
	const prefersReducedMotion =
		reducedMotionOverride ?? systemPrefersReducedMotion;
	const [expandedResourceId, setExpandedResourceId] = useState(() =>
		defaultExpanded(groups, pathname),
	);
	const groupsRef = useRef(groups);
	groupsRef.current = groups;
	const activeChildRef = useRef<HTMLAnchorElement | null>(null);
	const disclosureIdPrefix = useId().replaceAll(":", "");

	useEffect(() => {
		setExpandedResourceId(defaultExpanded(groupsRef.current, pathname));
	}, [pathname]);

	useEffect(() => {
		const activeItem = groups
			.flatMap((group) => group.items)
			.find(
				(item) =>
					item.children !== undefined && isRouteMatch(pathname, item.href),
			);
		if (!activeItem || expandedResourceId !== activeItem.id) return;
		const frame = window.requestAnimationFrame(() => {
			activeChildRef.current?.scrollIntoView({
				block: "nearest",
				inline: "nearest",
				behavior: "auto",
			});
		});
		return () => window.cancelAnimationFrame(frame);
	}, [expandedResourceId, groups, pathname]);

	function followLink(
		event: MouseEvent<HTMLAnchorElement>,
		href: string,
		navigate: (href: string) => void,
	) {
		if (
			event.defaultPrevented ||
			event.button !== 0 ||
			event.metaKey ||
			event.altKey ||
			event.ctrlKey ||
			event.shiftKey
		) {
			return;
		}
		event.preventDefault();
		navigate(href);
	}

	function toggleItem(item: ResourceNavigationItem) {
		const nextExpandedResourceId =
			expandedResourceId === item.id ? null : item.id;
		setExpandedResourceId(nextExpandedResourceId);
		if (nextExpandedResourceId !== null) {
			onResourceRequested?.(nextExpandedResourceId);
		}
	}

	return (
		<TooltipProvider delayDuration={500} skipDelayDuration={300}>
			<nav aria-label={ariaLabel} className="xp-panel p-4">
				<div className="space-y-6">
					{groups.map((group) => (
						<div key={group.title} className="space-y-2">
							<p className="px-2 text-xs uppercase tracking-[0.18em] text-muted-foreground">
								{group.title}
							</p>
							<ul className="space-y-1.5">
								{group.items.map((item) => {
									const isResource = item.children !== undefined;
									const childrenHaveLeadingIcons =
										(item.children?.length ?? 0) > 0 &&
										item.children?.every(
											(child) => child.leadingIcon !== undefined,
										);
									const isExpanded = expandedResourceId === item.id;
									const isActive = isRouteMatch(pathname, item.href);
									const panelId = `${disclosureIdPrefix}-${item.id}-children`;
									return (
										<li key={item.id} className="space-y-1">
											<div className="flex items-center gap-1">
												<a
													href={item.href}
													aria-current={
														pathname === item.href ? "page" : undefined
													}
													className={[
														primaryLinkClass,
														isActive
															? "border-primary/25 bg-primary/10 text-foreground shadow-sm"
															: "",
													]
														.filter(Boolean)
														.join(" ")}
													onClick={(event) =>
														followLink(event, item.href, onNavigate)
													}
												>
													<Icon
														name={item.icon}
														className="size-5 shrink-0 opacity-80"
													/>
													<span className="truncate">{item.label}</span>
												</a>
												{isResource ? (
													<button
														type="button"
														aria-label={`${isExpanded ? "Collapse" : "Expand"} ${item.label}`}
														aria-controls={panelId}
														aria-expanded={isExpanded}
														className={toggleButtonClass}
														onClick={() => toggleItem(item)}
													>
														<Icon
															name={
																isExpanded
																	? "tabler:chevron-down"
																	: "tabler:chevron-right"
															}
															ariaLabel={`${isExpanded ? "Collapse" : "Expand"} ${item.label}`}
															className="size-4"
														/>
													</button>
												) : null}
											</div>
											{isResource && isExpanded ? (
												<div
													id={panelId}
													className={
														childrenHaveLeadingIcons ? undefined : "pl-5"
													}
												>
													{item.isLoading ? (
														<p className="px-3 py-2 text-xs text-muted-foreground">
															Loading {item.label.toLowerCase()}...
														</p>
													) : item.error ? (
														<div className="flex items-center gap-2 px-3 py-2 text-xs text-destructive">
															<span className="min-w-0 truncate">
																Unable to load
															</span>
															{item.onRetry ? (
																<button
																	type="button"
																	aria-label={`Retry ${item.label}`}
																	className={retryButtonClass}
																	onClick={item.onRetry}
																>
																	<Icon
																		name="tabler:refresh"
																		ariaLabel={`Retry ${item.label}`}
																		className="size-4"
																	/>
																</button>
															) : null}
														</div>
													) : (item.children ?? []).length === 0 ? (
														<p className="px-3 py-2 text-xs text-muted-foreground">
															No {item.label.toLowerCase()} yet
														</p>
													) : (
														<ScrollArea
															data-testid={`resource-list-${item.id}`}
															className="max-h-[20rem] [&_[data-radix-scroll-area-viewport]]:max-h-[20rem]"
														>
															<ul className="w-0 min-w-full pr-1">
																{item.children?.map((child) => {
																	const isChildActive = isRouteMatch(
																		pathname,
																		child.href,
																	);
																	return (
																		<li key={child.id} className="min-w-0">
																			<ResourceNavigationChildLink
																				ref={
																					isChildActive ? activeChildRef : null
																				}
																				href={child.href}
																				label={child.label}
																				leadingIcon={child.leadingIcon}
																				isActive={isChildActive}
																				prefersReducedMotion={
																					prefersReducedMotion
																				}
																				aria-label={child.ariaLabel}
																				aria-current={
																					isChildActive ? "page" : undefined
																				}
																				onClick={(event) =>
																					followLink(
																						event,
																						child.href,
																						onResourceNavigate ?? onNavigate,
																					)
																				}
																			/>
																		</li>
																	);
																})}
															</ul>
														</ScrollArea>
													)}
												</div>
											) : null}
										</li>
									);
								})}
							</ul>
						</div>
					))}
				</div>
			</nav>
		</TooltipProvider>
	);
}
