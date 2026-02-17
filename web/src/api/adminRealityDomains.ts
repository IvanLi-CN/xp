import { z } from "zod";

import { throwIfNotOk } from "./backendError";

export const AdminRealityDomainSchema = z.object({
	domain_id: z.string(),
	server_name: z.string(),
	disabled_node_ids: z.array(z.string()).default([]),
});

export type AdminRealityDomain = z.infer<typeof AdminRealityDomainSchema>;

export const AdminRealityDomainsResponseSchema = z.object({
	items: z.array(AdminRealityDomainSchema),
});

export type AdminRealityDomainsResponse = z.infer<
	typeof AdminRealityDomainsResponseSchema
>;

export type AdminRealityDomainCreateRequest = {
	server_name: string;
	disabled_node_ids?: string[];
};

export type AdminRealityDomainPatchRequest = {
	server_name?: string;
	disabled_node_ids?: string[];
};

export async function fetchAdminRealityDomains(
	adminToken: string,
	signal?: AbortSignal,
): Promise<AdminRealityDomainsResponse> {
	const res = await fetch("/api/admin/reality-domains", {
		method: "GET",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminRealityDomainsResponseSchema.parse(json);
}

export async function createAdminRealityDomain(
	adminToken: string,
	payload: AdminRealityDomainCreateRequest,
	signal?: AbortSignal,
): Promise<AdminRealityDomain> {
	const res = await fetch("/api/admin/reality-domains", {
		method: "POST",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
			"Content-Type": "application/json",
		},
		body: JSON.stringify(payload),
		signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminRealityDomainSchema.parse(json);
}

export async function patchAdminRealityDomain(
	adminToken: string,
	domainId: string,
	payload: AdminRealityDomainPatchRequest,
	signal?: AbortSignal,
): Promise<AdminRealityDomain> {
	const res = await fetch(`/api/admin/reality-domains/${domainId}`, {
		method: "PATCH",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
			"Content-Type": "application/json",
		},
		body: JSON.stringify(payload),
		signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminRealityDomainSchema.parse(json);
}

export async function deleteAdminRealityDomain(
	adminToken: string,
	domainId: string,
	signal?: AbortSignal,
): Promise<void> {
	const res = await fetch(`/api/admin/reality-domains/${domainId}`, {
		method: "DELETE",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
		},
		signal,
	});

	await throwIfNotOk(res);
}

export async function reorderAdminRealityDomains(
	adminToken: string,
	domainIds: string[],
	signal?: AbortSignal,
): Promise<void> {
	const res = await fetch("/api/admin/reality-domains/reorder", {
		method: "POST",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
			"Content-Type": "application/json",
		},
		body: JSON.stringify({ domain_ids: domainIds }),
		signal,
	});

	await throwIfNotOk(res);
}
