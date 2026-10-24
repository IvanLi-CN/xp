import { throwIfNotOk } from "./backendError";

export type SubscriptionFormat = "raw" | "clash" | "mihomo";

export type SubscriptionFormatOption = {
	value: SubscriptionFormat;
	label: string;
};

export const SUBSCRIPTION_FORMAT_OPTIONS = [
	{ value: "raw", label: "Raw" },
	{ value: "clash", label: "Clash" },
	{ value: "mihomo", label: "Mihomo" },
] as const satisfies readonly SubscriptionFormatOption[];

export const DEFAULT_SUBSCRIPTION_FORMAT: SubscriptionFormat = "raw";

export type SubscriptionUrlOptions = {
	origin?: string;
	subscriptionToken: string;
	format?: SubscriptionFormat;
	externalResources?: "mirror";
};

export function buildSubscriptionUrl({
	origin,
	subscriptionToken,
	format = DEFAULT_SUBSCRIPTION_FORMAT,
	externalResources,
}: SubscriptionUrlOptions): string {
	if (!subscriptionToken) return "";

	const path = `/api/sub/${encodeURIComponent(subscriptionToken)}`;
	const params = new URLSearchParams({ format });
	if (format === "mihomo" && externalResources) {
		params.set("external_resources", externalResources);
	}
	const relativeUrl = `${path}?${params.toString()}`;
	return origin ? new URL(relativeUrl, origin).toString() : relativeUrl;
}

export async function fetchSubscription(
	subscriptionToken: string,
	format?: SubscriptionFormat,
	externalResources?: "mirror",
	signal?: AbortSignal,
): Promise<string> {
	const params = new URLSearchParams();
	let url = `/api/sub/${encodeURIComponent(subscriptionToken)}`;
	if (format) {
		params.set("format", format);
	}
	if (externalResources) {
		params.set("external_resources", externalResources);
	}
	const query = params.toString();
	if (query) {
		url = `${url}?${query}`;
	}

	const res = await fetch(url, {
		method: "GET",
		headers: {
			Accept: "text/plain",
		},
		cache: "no-store",
		signal,
	});

	await throwIfNotOk(res);

	return res.text();
}
