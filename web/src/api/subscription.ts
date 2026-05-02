import { throwIfNotOk } from "./backendError";

export type SubscriptionFormat = "raw" | "clash" | "mihomo";

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
