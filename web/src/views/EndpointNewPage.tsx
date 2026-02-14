import { useMutation, useQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { useEffect, useState } from "react";

import {
	type AdminEndpointKind,
	AdminEndpointKindSchema,
	createAdminEndpoint,
} from "../api/adminEndpoints";
import { fetchAdminNodes } from "../api/adminNodes";
import { isBackendApiError } from "../api/backendError";
import { Button } from "../components/Button";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
import { TagInput } from "../components/TagInput";
import { useToast } from "../components/Toast";
import { useUiPrefs } from "../components/UiPrefs";
import { readAdminToken } from "../components/auth";

const kindOptions = [
	{
		value: "vless_reality_vision_tcp" as const,
		label: "VLESS Reality Vision TCP",
	},
	{
		value: "ss2022_2022_blake3_aes_128_gcm" as const,
		label: "SS2022 BLAKE3 AES-128-GCM",
	},
];

function formatErrorMessage(error: unknown): string {
	if (isBackendApiError(error)) {
		const code = error.code ? ` ${error.code}` : "";
		return `${error.status}${code}: ${error.message}`;
	}
	if (error instanceof Error) return error.message;
	return String(error);
}

function normalizeRealityServerName(value: string): string {
	return value.trim();
}

function isValidRealityServerName(value: string): boolean {
	if (!value) return false;
	if (/\s/.test(value)) return false;
	if (value.includes("://")) return false;
	if (value.includes("/")) return false;
	if (value.includes(":")) return false;
	return true;
}

function validateRealityServerName(value: string): string | null {
	const trimmed = normalizeRealityServerName(value);
	if (!trimmed) return "serverName is required.";
	if (!isValidRealityServerName(trimmed)) {
		return "serverName must be a domain (no scheme/path/port).";
	}
	if (trimmed.includes("*")) return "Wildcard is not supported.";
	return null;
}

export function EndpointNewPage() {
	const navigate = useNavigate();
	const { pushToast } = useToast();
	const prefs = useUiPrefs();
	const adminToken = readAdminToken();

	const selectClass =
		prefs.density === "compact"
			? "select select-bordered select-sm"
			: "select select-bordered";
	const inputClass =
		prefs.density === "compact"
			? "input input-bordered input-sm"
			: "input input-bordered";

	const nodesQuery = useQuery({
		queryKey: ["adminNodes", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminNodes(adminToken, signal),
	});

	const [kind, setKind] = useState<AdminEndpointKind>(
		"vless_reality_vision_tcp",
	);
	const [nodeId, setNodeId] = useState("");
	const [port, setPort] = useState("443");
	const [realityServerNames, setRealityServerNames] = useState<string[]>([]);
	const [realityFingerprint, setRealityFingerprint] = useState("chrome");

	useEffect(() => {
		const nodes = nodesQuery.data?.items ?? [];
		if (nodes.length === 0) return;
		if (!nodeId || !nodes.some((node) => node.node_id === nodeId)) {
			setNodeId(nodes[0].node_id);
		}
	}, [nodeId, nodesQuery.data]);

	const createMutation = useMutation({
		mutationFn: async () => {
			const portNumber = Number.parseInt(port, 10);
			if (!Number.isFinite(portNumber) || portNumber <= 0) {
				throw new Error("Please enter a valid port.");
			}
			if (adminToken.length === 0) {
				throw new Error("Missing admin token.");
			}

			if (kind === "vless_reality_vision_tcp") {
				const serverNames = realityServerNames
					.map(normalizeRealityServerName)
					.filter((s) => s.length > 0);
				if (serverNames.length === 0)
					throw new Error("serverName is required.");
				for (const name of serverNames) {
					const err = validateRealityServerName(name);
					if (err) throw new Error(err);
				}

				const fingerprintValue = realityFingerprint.trim() || "chrome";
				const primary = serverNames[0];

				return createAdminEndpoint(adminToken, {
					kind,
					node_id: nodeId,
					port: portNumber,
					reality: {
						dest: `${primary}:443`,
						server_names: serverNames,
						fingerprint: fingerprintValue,
					},
				});
			}

			return createAdminEndpoint(adminToken, {
				kind,
				node_id: nodeId,
				port: portNumber,
			});
		},
		onSuccess: (endpoint) => {
			pushToast({
				variant: "success",
				message: "Endpoint created successfully.",
			});
			navigate({
				to: "/endpoints/$endpointId",
				params: { endpointId: endpoint.endpoint_id },
			});
		},
		onError: (error) => {
			pushToast({
				variant: "error",
				message: formatErrorMessage(error),
			});
		},
	});

	const content = (() => {
		if (adminToken.length === 0) {
			return (
				<PageState
					variant="empty"
					title="Admin token required"
					description="Set an admin token to create endpoints."
					action={
						<Link className="btn btn-primary" to="/login">
							Go to login
						</Link>
					}
				/>
			);
		}

		if (nodesQuery.isLoading) {
			return (
				<PageState
					variant="loading"
					title="Loading nodes"
					description="Fetching nodes for endpoint assignment."
				/>
			);
		}

		if (nodesQuery.isError) {
			const description = formatErrorMessage(nodesQuery.error);
			return (
				<PageState
					variant="error"
					title="Failed to load nodes"
					description={description}
					action={
						<Button variant="secondary" onClick={() => nodesQuery.refetch()}>
							Retry
						</Button>
					}
				/>
			);
		}

		const nodes = nodesQuery.data?.items ?? [];
		if (nodes.length === 0) {
			return (
				<PageState
					variant="empty"
					title="No nodes available"
					description="Create or register a node before adding endpoints."
					action={
						<Link className="btn btn-primary" to="/nodes">
							Go to nodes
						</Link>
					}
				/>
			);
		}

		return (
			<div className="card bg-base-100 shadow">
				<div className="card-body space-y-4">
					<form
						className="space-y-4"
						onSubmit={(event) => {
							event.preventDefault();
							createMutation.mutate();
						}}
					>
						<div className="grid gap-4 md:grid-cols-2">
							<label className="form-control">
								<div className="label">
									<span className="label-text">Kind</span>
								</div>
								<select
									className={selectClass}
									value={kind}
									onChange={(event) =>
										setKind(AdminEndpointKindSchema.parse(event.target.value))
									}
								>
									{kindOptions.map((option) => (
										<option key={option.value} value={option.value}>
											{option.label}
										</option>
									))}
								</select>
							</label>

							{nodes.length <= 1 ? (
								<div className="form-control">
									<div className="label">
										<span className="label-text">Node</span>
									</div>
									<div className="rounded-lg border border-base-300 bg-base-200 px-3 py-3 text-sm">
										<span className="font-medium">
											{nodes[0]?.node_name ?? "Node"}
										</span>{" "}
										<span className="font-mono text-xs opacity-70">
											({nodes[0]?.node_id ?? nodeId})
										</span>
									</div>
								</div>
							) : (
								<label className="form-control">
									<div className="label">
										<span className="label-text">Node</span>
									</div>
									<select
										className={selectClass}
										value={nodeId}
										onChange={(event) => setNodeId(event.target.value)}
									>
										{nodes.map((node) => (
											<option key={node.node_id} value={node.node_id}>
												{node.node_name} ({node.node_id})
											</option>
										))}
									</select>
								</label>
							)}
						</div>

						{kind === "vless_reality_vision_tcp" ? (
							<div className="space-y-4 border-t border-base-200 pt-4">
								<h2 className="text-lg font-semibold">VLESS settings</h2>
								<div className="grid gap-4 md:grid-cols-2">
									<label className="form-control">
										<div className="label">
											<span className="label-text font-mono">port</span>
										</div>
										<input
											type="number"
											className={inputClass}
											value={port}
											min={1}
											onChange={(event) => setPort(event.target.value)}
										/>
										<p className="text-xs opacity-70">
											The inbound listen port on this node.
										</p>
									</label>
									<TagInput
										label="serverNames"
										value={realityServerNames}
										onChange={setRealityServerNames}
										placeholder="oneclient.sfx.ms"
										disabled={createMutation.isPending}
										inputClass={inputClass}
										validateTag={validateRealityServerName}
										helperText="Camouflage domains (TLS SNI). First tag is primary (used for dest/probe). Subscription may randomly output one of the tags."
									/>
									<details className="collapse collapse-arrow border border-base-200 bg-base-200/40 md:col-span-2">
										<summary className="collapse-title text-sm font-medium">
											Advanced (optional)
										</summary>
										<div className="collapse-content space-y-4">
											<label className="form-control">
												<div className="label">
													<span className="label-text font-mono">
														fingerprint
													</span>
												</div>
												<input
													type="text"
													className={inputClass}
													value={realityFingerprint}
													placeholder="chrome"
													onChange={(event) =>
														setRealityFingerprint(event.target.value)
													}
												/>
												<p className="text-xs opacity-70">
													Defaults to <span className="font-mono">chrome</span>.
												</p>
											</label>
										</div>
									</details>
								</div>
							</div>
						) : (
							<div className="space-y-4 border-t border-base-200 pt-4">
								<h2 className="text-lg font-semibold">SS2022 settings</h2>
								<div className="grid gap-4 md:grid-cols-2">
									<label className="form-control">
										<div className="label">
											<span className="label-text font-mono">port</span>
										</div>
										<input
											type="number"
											className={inputClass}
											value={port}
											min={1}
											onChange={(event) => setPort(event.target.value)}
										/>
										<p className="text-xs opacity-70">
											The inbound listen port on this node.
										</p>
									</label>
								</div>
							</div>
						)}

						<div className="card-actions justify-end gap-2">
							<Link className="btn btn-ghost" to="/endpoints">
								Cancel
							</Link>
							<Button
								loading={createMutation.isPending}
								disabled={createMutation.isPending}
								type="submit"
							>
								Create endpoint
							</Button>
						</div>
					</form>
				</div>
			</div>
		);
	})();

	return (
		<div className="space-y-6">
			<PageHeader
				title="New endpoint"
				description="Create an ingress endpoint for a node."
				actions={
					<Link className="btn btn-ghost btn-sm" to="/endpoints">
						Back
					</Link>
				}
			/>
			{content}
		</div>
	);
}
