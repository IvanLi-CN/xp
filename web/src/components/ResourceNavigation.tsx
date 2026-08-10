import type { MouseEvent } from "react";
import { useEffect, useId, useRef, useState } from "react";

import { Icon } from "./Icon";

export type ResourceNavigationChild = {
	id: string;
	label: string;
	href: string;
	ariaLabel: string;
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
};

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

const childLinkClass = [
	"flex h-8 items-center rounded-lg px-3 text-xs font-medium text-muted-foreground",
	"transition-colors hover:bg-muted hover:text-foreground",
].join(" ");

function isRouteMatch(pathname: string, href: string) {
	return pathname === href || pathname.startsWith(`${href}/`);
}

function defaultExpanded(groups: ResourceNavigationGroup[], pathname: string) {
	return Object.fromEntries(
		groups.flatMap((group) =>
			group.items
				.filter((item) => item.children !== undefined)
				.map((item) => [item.id, pathname.startsWith(`${item.href}/`)]),
		),
	) as Record<string, boolean>;
}

export function ResourceNavigation({
	ariaLabel,
	groups,
	pathname,
	onNavigate,
	onResourceNavigate,
	onResourceRequested,
}: ResourceNavigationProps) {
	const [expanded, setExpanded] = useState(() =>
		defaultExpanded(groups, pathname),
	);
	const groupsRef = useRef(groups);
	groupsRef.current = groups;
	const activeChildRef = useRef<HTMLAnchorElement | null>(null);
	const disclosureIdPrefix = useId().replaceAll(":", "");

	useEffect(() => {
		setExpanded(defaultExpanded(groupsRef.current, pathname));
	}, [pathname]);

	useEffect(() => {
		const activeItem = groups
			.flatMap((group) => group.items)
			.find(
				(item) =>
					item.children !== undefined && isRouteMatch(pathname, item.href),
			);
		if (!activeItem || !expanded[activeItem.id]) return;
		const frame = window.requestAnimationFrame(() => {
			activeChildRef.current?.scrollIntoView({
				block: "nearest",
				inline: "nearest",
				behavior: "auto",
			});
		});
		return () => window.cancelAnimationFrame(frame);
	}, [expanded, groups, pathname]);

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
		const nextValue = !expanded[item.id];
		setExpanded((current) => {
			return { ...current, [item.id]: nextValue };
		});
		if (nextValue) onResourceRequested?.(item.id);
	}

	return (
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
								const isExpanded = expanded[item.id] ?? false;
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
											<div id={panelId} className="pl-5">
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
													<ul
														data-testid={`resource-list-${item.id}`}
														className="max-h-[20rem] overflow-y-auto pr-1"
													>
														{item.children?.map((child) => {
															const isChildActive = isRouteMatch(
																pathname,
																child.href,
															);
															return (
																<li key={child.id}>
																	<a
																		ref={isChildActive ? activeChildRef : null}
																		href={child.href}
																		title={child.ariaLabel}
																		aria-label={child.ariaLabel}
																		aria-current={
																			isChildActive ? "page" : undefined
																		}
																		className={[
																			childLinkClass,
																			isChildActive
																				? "bg-primary/10 text-foreground"
																				: "",
																		]
																			.filter(Boolean)
																			.join(" ")}
																		onClick={(event) =>
																			followLink(
																				event,
																				child.href,
																				onResourceNavigate ?? onNavigate,
																			)
																		}
																	>
																		<span className="truncate">
																			{child.label}
																		</span>
																	</a>
																</li>
															);
														})}
													</ul>
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
	);
}
