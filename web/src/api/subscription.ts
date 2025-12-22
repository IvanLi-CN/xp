import { throwIfNotOk } from "./backendError";

export type SubscriptionFormat = "raw" | "clash";

export async function fetchSubscription(
	subscriptionToken: string,
	format?: SubscriptionFormat,
	signal?: AbortSignal,
): Promise<string> {
	const params = new URLSearchParams();
	if (format) {
		params.set("format", format);
	}
	const query = params.toString();
	const url = query
		? `/api/sub/${encodeURIComponent(subscriptionToken)}?${query}`
		: `/api/sub/${encodeURIComponent(subscriptionToken)}`;

	const res = await fetch(url, {
		method: "GET",
		headers: {
			Accept: "text/plain",
		},
		signal,
	});

	await throwIfNotOk(res);

	return res.text();
}
