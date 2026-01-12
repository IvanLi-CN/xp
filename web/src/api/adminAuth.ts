import { throwIfNotOk } from "./backendError";

export async function verifyAdminToken(
	adminToken: string,
	signal?: AbortSignal,
): Promise<void> {
	const res = await fetch("/api/admin/alerts", {
		method: "GET",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});

	await throwIfNotOk(res);
}
