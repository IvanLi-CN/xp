import { readAdminToken } from "../components/auth";

export type NodePageLink = {
	href: string;
	disabled: false;
};

export type DisabledNodePageLink = {
	reason: string;
	disabled: true;
};

export type ResolvedNodePageLink = NodePageLink | DisabledNodePageLink;

export function sanitizeRedirectPath(value: unknown): string {
	if (typeof value !== "string") return "/";
	if (!value.startsWith("/")) return "/";
	if (value.startsWith("//")) return "/";
	return value;
}

export function stripLoginToken(
	searchParams: URLSearchParams,
): URLSearchParams {
	const next = new URLSearchParams(searchParams);
	next.delete("login_token");
	return next;
}

export function buildRedirectTarget(
	pathname: string,
	search: string,
	hash: string,
): string {
	const searchParams = stripLoginToken(new URLSearchParams(search));
	const nextSearch = searchParams.toString();
	const normalizedHash = hash ? (hash.startsWith("#") ? hash : `#${hash}`) : "";

	return `${pathname}${nextSearch.length > 0 ? `?${nextSearch}` : ""}${normalizedHash}`;
}

export function resolveLoginRedirectFromHref(href: string): string {
	try {
		const current = new URL(href, window.location.origin);
		return buildRedirectTarget(current.pathname, current.search, current.hash);
	} catch {
		return "/";
	}
}

export function buildNodePageHref(
	apiBaseUrl: string,
	currentHref: string = window.location.href,
	adminToken: string = readAdminToken(),
): ResolvedNodePageLink {
	const trimmed = apiBaseUrl.trim();
	if (trimmed.length === 0) {
		return {
			disabled: true,
			reason: "Open on node unavailable: API base URL is empty.",
		};
	}

	let baseUrl: URL;
	try {
		baseUrl = new URL(trimmed);
	} catch {
		return {
			disabled: true,
			reason: "Open on node unavailable: API base URL is invalid.",
		};
	}

	if (baseUrl.protocol !== "https:") {
		return {
			disabled: true,
			reason: "Open on node unavailable: API base URL must use HTTPS.",
		};
	}

	let currentUrl: URL;
	try {
		currentUrl = new URL(currentHref);
	} catch {
		return {
			disabled: true,
			reason: "Open on node unavailable: current page URL is invalid.",
		};
	}

	const nextUrl = new URL(currentUrl.pathname, baseUrl.origin);
	const nextSearchParams = stripLoginToken(currentUrl.searchParams);
	if (adminToken.length > 0) {
		nextSearchParams.set("login_token", adminToken);
	}

	nextUrl.search = nextSearchParams.toString();
	nextUrl.hash = currentUrl.hash;

	return {
		disabled: false,
		href: nextUrl.toString(),
	};
}
