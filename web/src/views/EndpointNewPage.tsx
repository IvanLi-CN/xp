import { useMutation, useQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { useEffect, useMemo, useState } from "react";

import {
	type AdminEndpointKind,
	AdminEndpointKindSchema,
	createAdminEndpoint,
} from "../api/adminEndpoints";
import { fetchAdminNodes } from "../api/adminNodes";
import { isBackendApiError } from "../api/backendError";
import { Button } from "../components/Button";
import { PageState } from "../components/PageState";
import { useToast } from "../components/Toast";
import { readAdminToken } from "../components/auth";

function formatErrorMessage(error: unknown): string {
	if (isBackendApiError(error)) {
		const code = error.code ? ` ${error.code}` : "";
		return `${error.status}${code}: ${error.message}`;
	}
	if (error instanceof Error) return error.message;
	return String(error);
}

function parseServerNames(value: string): string[] {
	return value
		.split(",")
		.map((entry) => entry.trim())
		.filter((entry) => entry.length > 0);
}

export function EndpointNewPage() {
	const navigate = useNavigate();
	const { pushToast } = useToast();
	const adminToken = readAdminToken();

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
	const [publicDomain, setPublicDomain] = useState("");
	const [realityDest, setRealityDest] = useState("");
	const [realityServerNames, setRealityServerNames] = useState("");
	const [realityFingerprint, setRealityFingerprint] = useState("");

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
				const publicDomainTrimmed = publicDomain.trim();
				const destTrimmed = realityDest.trim();
				const fingerprintTrimmed = realityFingerprint.trim();
				const serverNames = parseServerNames(realityServerNames);
				if (!publicDomainTrimmed) {
					throw new Error("Public domain is required for VLESS endpoints.");
				}
				if (!destTrimmed) {
					throw new Error("Reality destination is required.");
				}
				if (serverNames.length === 0) {
					throw new Error("Provide at least one reality server name.");
				}
				if (!fingerprintTrimmed) {
					throw new Error("Reality fingerprint is required.");
				}

				return createAdminEndpoint(adminToken, {
					kind,
					node_id: nodeId,
					port: portNumber,
					public_domain: publicDomainTrimmed,
					reality: {
						dest: destTrimmed,
						server_names: serverNames,
						fingerprint: fingerprintTrimmed,
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

	const kindOptions = useMemo(
		() => [
			{
				value: "vless_reality_vision_tcp" as const,
				label: "VLESS Reality Vision TCP",
			},
			{
				value: "ss2022_2022_blake3_aes_128_gcm" as const,
				label: "SS2022 BLAKE3 AES-128-GCM",
			},
		],
		[],
	);

	return (
		<div className="space-y-6">
			<div>
				<h1 className="text-2xl font-bold">New endpoint</h1>
				<p className="text-sm opacity-70">
					Create an ingress endpoint for a node.
				</p>
			</div>

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
									className="select select-bordered"
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
							<label className="form-control">
								<div className="label">
									<span className="label-text">Node</span>
								</div>
								<select
									className="select select-bordered"
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
							<label className="form-control">
								<div className="label">
									<span className="label-text">Port</span>
								</div>
								<input
									type="number"
									className="input input-bordered"
									value={port}
									min={1}
									onChange={(event) => setPort(event.target.value)}
								/>
							</label>
						</div>

						{kind === "vless_reality_vision_tcp" ? (
							<div className="space-y-4 border-t border-base-200 pt-4">
								<h2 className="text-lg font-semibold">VLESS settings</h2>
								<div className="grid gap-4 md:grid-cols-2">
									<label className="form-control md:col-span-2">
										<div className="label">
											<span className="label-text">Public domain</span>
										</div>
										<input
											type="text"
											className="input input-bordered"
											value={publicDomain}
											placeholder="example.com"
											onChange={(event) => setPublicDomain(event.target.value)}
										/>
									</label>
									<label className="form-control md:col-span-2">
										<div className="label">
											<span className="label-text">Reality destination</span>
										</div>
										<input
											type="text"
											className="input input-bordered"
											value={realityDest}
											placeholder="example.com:443"
											onChange={(event) => setRealityDest(event.target.value)}
										/>
									</label>
									<label className="form-control md:col-span-2">
										<div className="label">
											<span className="label-text">Reality server names</span>
										</div>
										<input
											type="text"
											className="input input-bordered"
											value={realityServerNames}
											placeholder="example.com, edge.example.com"
											onChange={(event) =>
												setRealityServerNames(event.target.value)
											}
										/>
										<p className="text-xs opacity-70">
											Comma-separated list of server names.
										</p>
									</label>
									<label className="form-control md:col-span-2">
										<div className="label">
											<span className="label-text">Fingerprint</span>
										</div>
										<input
											type="text"
											className="input input-bordered"
											value={realityFingerprint}
											placeholder="chrome"
											onChange={(event) =>
												setRealityFingerprint(event.target.value)
											}
										/>
									</label>
								</div>
							</div>
						) : null}

						<div className="card-actions justify-end">
							<Button loading={createMutation.isPending} type="submit">
								Create endpoint
							</Button>
						</div>
					</form>
				</div>
			</div>
		</div>
	);
}
