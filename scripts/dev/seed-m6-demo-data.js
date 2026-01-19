#!/usr/bin/env node
"use strict";

const baseUrl = (process.env.XP_API_BASE_URL || "http://127.0.0.1:62416").replace(
	/\/+$/,
	"",
);
const adminToken = process.env.XP_ADMIN_TOKEN || "testtoken";

const headers = {
	Accept: "application/json",
	Authorization: `Bearer ${adminToken}`,
};

function apiUrl(path) {
	if (!path.startsWith("/")) {
		throw new Error(`API path must start with '/': ${path}`);
	}
	return `${baseUrl}${path}`;
}

async function requestJson(path, options = {}) {
	const res = await fetch(apiUrl(path), {
		...options,
		headers: {
			...headers,
			...(options.headers || {}),
		},
	});

	if (!res.ok) {
		const text = await res.text();
		throw new Error(
			`Request failed ${options.method || "GET"} ${path} -> ${res.status}: ${text}`,
		);
	}

	if (res.status === 204) {
		return null;
	}

	return res.json();
}

async function getNodes() {
	return requestJson("/api/admin/nodes");
}

async function getUsers() {
	return requestJson("/api/admin/users");
}

async function getEndpoints() {
	return requestJson("/api/admin/endpoints");
}

async function getGrants() {
	return requestJson("/api/admin/grants");
}

async function createUser(payload) {
	return requestJson("/api/admin/users", {
		method: "POST",
		headers: { "Content-Type": "application/json" },
		body: JSON.stringify(payload),
	});
}

async function createEndpoint(payload) {
	return requestJson("/api/admin/endpoints", {
		method: "POST",
		headers: { "Content-Type": "application/json" },
		body: JSON.stringify(payload),
	});
}

async function createGrant(payload) {
	return requestJson("/api/admin/grants", {
		method: "POST",
		headers: { "Content-Type": "application/json" },
		body: JSON.stringify(payload),
	});
}

async function patchGrant(grantId, payload) {
	return requestJson(`/api/admin/grants/${grantId}`, {
		method: "PATCH",
		headers: { "Content-Type": "application/json" },
		body: JSON.stringify(payload),
	});
}

function formatBool(value) {
	return value ? "true" : "false";
}

function summarizeGrant(grant) {
	return `grant_id=${grant.grant_id} user_id=${grant.user_id} endpoint_id=${grant.endpoint_id} enabled=${formatBool(
		grant.enabled,
	)} note=${grant.note === null ? "null" : JSON.stringify(grant.note)}`;
}

async function main() {
	const summary = {
		nodeId: null,
		users: [],
		endpoints: [],
		grants: [],
		grantPatches: [],
	};

	const nodes = await getNodes();
	if (!nodes.items || nodes.items.length === 0) {
		throw new Error(
			"No nodes found. Start backend and ensure /api/admin/nodes returns at least one node.",
		);
	}

	const node = nodes.items[0];
	summary.nodeId = node.node_id;

	const users = await getUsers();
	const endpoints = await getEndpoints();
	const grants = await getGrants();

	const desiredUsers = [
		{
			display_name: "M6 Demo Alice",
			cycle_policy_default: "by_user",
			cycle_day_of_month_default: 1,
		},
		{
			display_name: "M6 Demo Bob",
			cycle_policy_default: "by_node",
			cycle_day_of_month_default: 15,
		},
	];

	const ensuredUsers = new Map();

	for (const payload of desiredUsers) {
		const existing = users.items.find(
			(item) => item.display_name === payload.display_name,
		);

		if (existing) {
			ensuredUsers.set(payload.display_name, existing);
			summary.users.push({ status: "reused", user: existing });
			continue;
		}

		const created = await createUser(payload);
		ensuredUsers.set(payload.display_name, created);
		users.items.push(created);
		summary.users.push({ status: "created", user: created });
	}

	const vlessConfig = {
		kind: "vless_reality_vision_tcp",
		node_id: node.node_id,
		port: 443,
		reality: {
			dest: "demo.example.com:443",
			server_names: ["demo.example.com"],
			fingerprint: "chrome",
		},
	};

	const ssConfig = {
		kind: "ss2022_2022_blake3_aes_128_gcm",
		node_id: node.node_id,
		port: 8388,
	};

	const desiredEndpoints = [vlessConfig, ssConfig];
	const ensuredEndpoints = new Map();

	for (const payload of desiredEndpoints) {
		const existing = endpoints.items.find(
			(item) => item.node_id === payload.node_id && item.kind === payload.kind,
		);

		if (existing) {
			ensuredEndpoints.set(payload.kind, existing);
			summary.endpoints.push({ status: "reused", endpoint: existing });
			continue;
		}

		const created = await createEndpoint(payload);
		ensuredEndpoints.set(payload.kind, created);
		endpoints.items.push(created);
		summary.endpoints.push({ status: "created", endpoint: created });
	}

	const alice = ensuredUsers.get("M6 Demo Alice");
	const bob = ensuredUsers.get("M6 Demo Bob");
	const vlessEndpoint = ensuredEndpoints.get("vless_reality_vision_tcp");
	const ssEndpoint = ensuredEndpoints.get("ss2022_2022_blake3_aes_128_gcm");

	if (!alice || !bob || !vlessEndpoint || !ssEndpoint) {
		throw new Error("Failed to ensure users/endpoints required for grants.");
	}

	const desiredGrants = [
		{
			name: "alice-vless",
			user_id: alice.user_id,
			endpoint_id: vlessEndpoint.endpoint_id,
			quota_limit_bytes: 10 * 1024 * 1024 * 1024,
			cycle_policy: "inherit_user",
			cycle_day_of_month: null,
			note: "demo: alice -> vless",
			enabled: true,
		},
		{
			name: "bob-ss",
			user_id: bob.user_id,
			endpoint_id: ssEndpoint.endpoint_id,
			quota_limit_bytes: 5 * 1024 * 1024 * 1024,
			cycle_policy: "by_user",
			cycle_day_of_month: 15,
			note: null,
			enabled: false,
		},
	];

	const ensuredGrants = new Map();

	for (const payload of desiredGrants) {
		const existing = grants.items.find(
			(item) =>
				item.user_id === payload.user_id &&
				item.endpoint_id === payload.endpoint_id,
		);

		if (existing) {
			ensuredGrants.set(payload.name, existing);
			summary.grants.push({ status: "reused", grant: existing });
			continue;
		}

		const created = await createGrant({
			user_id: payload.user_id,
			endpoint_id: payload.endpoint_id,
			quota_limit_bytes: payload.quota_limit_bytes,
			cycle_policy: payload.cycle_policy,
			cycle_day_of_month: payload.cycle_day_of_month,
			note: payload.note,
		});

		ensuredGrants.set(payload.name, created);
		grants.items.push(created);
		summary.grants.push({ status: "created", grant: created });
	}

	for (const payload of desiredGrants) {
		const grant = ensuredGrants.get(payload.name);
		if (!grant) {
			continue;
		}

		const needsPatch =
			grant.enabled !== payload.enabled ||
			grant.quota_limit_bytes !== payload.quota_limit_bytes ||
			grant.cycle_policy !== payload.cycle_policy ||
			grant.cycle_day_of_month !== payload.cycle_day_of_month ||
			grant.note !== payload.note;

		if (!needsPatch) {
			continue;
		}

		const patched = await patchGrant(grant.grant_id, {
			enabled: payload.enabled,
			quota_limit_bytes: payload.quota_limit_bytes,
			cycle_policy: payload.cycle_policy,
			cycle_day_of_month: payload.cycle_day_of_month,
			note: payload.note,
		});

		ensuredGrants.set(payload.name, patched);
		summary.grantPatches.push({ before: grant, after: patched });
	}

	console.log("\nM6 demo seed summary");
	console.log(`Base URL: ${baseUrl}`);
	console.log(`Node: ${summary.nodeId}`);
	console.log("\nUsers:");
	for (const entry of summary.users) {
		console.log(
			`- ${entry.status} display_name=${entry.user.display_name} user_id=${entry.user.user_id}`,
		);
	}

	console.log("\nEndpoints:");
	for (const entry of summary.endpoints) {
		console.log(
			`- ${entry.status} kind=${entry.endpoint.kind} endpoint_id=${entry.endpoint.endpoint_id} node_id=${entry.endpoint.node_id}`,
		);
	}

	console.log("\nGrants:");
	for (const entry of summary.grants) {
		console.log(`- ${entry.status} ${summarizeGrant(entry.grant)}`);
	}

	if (summary.grantPatches.length > 0) {
		console.log("\nGrant patches:");
		for (const entry of summary.grantPatches) {
			console.log(
				`- updated ${entry.before.grant_id} -> enabled ${formatBool(
					entry.before.enabled,
				)} -> ${formatBool(entry.after.enabled)}, note ${
					entry.before.note === null
						? "null"
						: JSON.stringify(entry.before.note)
				} -> ${
					entry.after.note === null
						? "null"
						: JSON.stringify(entry.after.note)
				}`,
			);
		}
	}

	console.log("\nSuggested UI pages:");
	console.log("- /users");
	console.log("- /endpoints");
	console.log("- /grants");
	console.log("\nDone.");
}

main().catch((err) => {
	console.error("\nSeed failed:");
	console.error(err instanceof Error ? err.message : err);
	process.exitCode = 1;
});
