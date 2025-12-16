import { z } from "zod";

export const HealthResponseSchema = z.object({
	status: z.literal("ok"),
});

export type HealthResponse = z.infer<typeof HealthResponseSchema>;

export async function fetchHealth(
	signal?: AbortSignal,
): Promise<HealthResponse> {
	const res = await fetch("/api/health", {
		method: "GET",
		headers: { Accept: "application/json" },
		signal,
	});

	if (!res.ok) {
		throw new Error(`health request failed: ${res.status}`);
	}

	const json: unknown = await res.json();
	return HealthResponseSchema.parse(json);
}
