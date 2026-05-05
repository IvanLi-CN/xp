import { throwIfNotOk } from "./backendError";

export type SubscriptionFormat = "raw" | "clash" | "mihomo";

export type SubscriptionFormatOption = {
	value: SubscriptionFormat;
	label: string;
};

export const SUBSCRIPTION_FORMAT_OPTIONS = [
	{ value: "raw", label: "raw" },
	{ value: "clash", label: "clash" },
	{ value: "mihomo", label: "mihomo(provider)" },
] as const satisfies readonly SubscriptionFormatOption[];

export const DEFAULT_SUBSCRIPTION_FORMAT: SubscriptionFormat = "raw";

export async function fetchSubscription(
	subscriptionToken: string,
	format?: SubscriptionFormat,
	signal?: AbortSignal,
): Promise<string> {
	const params = new URLSearchParams();
	let url = `/api/sub/${encodeURIComponent(subscriptionToken)}`;
	if (format) {
		params.set("format", format);
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
