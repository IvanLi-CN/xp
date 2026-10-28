import { readAdminToken } from "../components/auth";

const LOGIN_HANDOFF_TOKEN_PARAM = "xp_login_token";
const LOGIN_HANDOFF_HASH_PARAM = "xp_redirect_hash";

export type NodePageLink = {
	href: string;
	disabled: false;
};

export type DisabledNodePageLink = {
	reason: string;
	disabled: true;
};

export type ResolvedNodePageLink = NodePageLink | DisabledNodePageLink;

export type NodePageLinkOptions = {
	loginHandoff?: "query" | "fragment";
};

export type LoginHandoff = {
	token: string;
	redirectHash: string;
};

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

export function parseLoginHandoffHash(hash: string): LoginHandoff | null {
	if (!hash.startsWith("#")) return null;
	const params = new URLSearchParams(hash.slice(1));
	const token = params.get(LOGIN_HANDOFF_TOKEN_PARAM);
	if (!token) return null;
	const redirectHash = params.get(LOGIN_HANDOFF_HASH_PARAM) ?? "";
	return {
		token,
		redirectHash: redirectHash.length > 0 ? redirectHash : "",
	};
}

export function buildLoginHandoffHash(
	token: string,
	redirectHash: string,
): string {
	const params = new URLSearchParams();
	params.set(LOGIN_HANDOFF_TOKEN_PARAM, token);
	params.set(LOGIN_HANDOFF_HASH_PARAM, redirectHash);
	return `#${params.toString()}`;
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
		const handoff = parseLoginHandoffHash(current.hash);
		return buildRedirectTarget(
			current.pathname,
			current.search,
			handoff?.redirectHash ?? current.hash,
		);
	} catch {
		return "/";
	}
}

export function buildNodePageHref(
	apiBaseUrl: string,
	currentHref: string = window.location.href,
	adminToken: string = readAdminToken(),
	options: NodePageLinkOptions = {},
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
	if (
		(baseUrl.pathname !== "" && baseUrl.pathname !== "/") ||
		baseUrl.search ||
		baseUrl.hash ||
		baseUrl.username ||
		baseUrl.password ||
		baseUrl.href.includes("@")
	) {
		return {
			disabled: true,
			reason: "Open on node unavailable: API base URL must be an origin.",
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
		if (options.loginHandoff === "fragment") {
			nextUrl.hash = buildLoginHandoffHash(adminToken, currentUrl.hash);
		} else {
			nextSearchParams.set("login_token", adminToken);
		}
	}

	nextUrl.search = nextSearchParams.toString();
	if (options.loginHandoff !== "fragment" || adminToken.length === 0) {
		nextUrl.hash = currentUrl.hash;
	}

	return {
		disabled: false,
		href: nextUrl.toString(),
	};
}
