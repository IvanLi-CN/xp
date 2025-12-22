import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate, useParams } from "@tanstack/react-router";
import { useEffect, useState } from "react";

import {
	deleteAdminEndpoint,
	fetchAdminEndpoint,
	patchAdminEndpoint,
	rotateAdminEndpointShortId,
} from "../api/adminEndpoints";
import { isBackendApiError } from "../api/backendError";
import { Button } from "../components/Button";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { PageState } from "../components/PageState";
import { useToast } from "../components/Toast";
import { readAdminToken } from "../components/auth";

type VlessMetaSnapshot = {
	publicDomain: string;
	realityDest: string;
	realityServerNames: string[];
	realityFingerprint: string;
};

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

function isRecord(value: unknown): value is Record<string, unknown> {
	return typeof value === "object" && value !== null;
}

function asString(value: unknown): string | undefined {
	return typeof value === "string" ? value : undefined;
}

function asStringArray(value: unknown): string[] | undefined {
	if (!Array.isArray(value)) return undefined;
	const filtered = value.filter((entry) => typeof entry === "string");
	return filtered.length === value.length ? filtered : undefined;
}

function parseVlessMeta(meta: Record<string, unknown>): VlessMetaSnapshot {
	const publicDomain = asString(meta.public_domain) ?? "";
	const reality = isRecord(meta.reality) ? meta.reality : undefined;
	const realityDest = asString(reality?.dest) ?? "";
	const realityServerNames = asStringArray(reality?.server_names) ?? [];
	const realityFingerprint = asString(reality?.fingerprint) ?? "";

	return {
		publicDomain,
		realityDest,
		realityServerNames,
		realityFingerprint,
	};
}

function arraysEqual(left: string[], right: string[]): boolean {
	if (left.length !== right.length) return false;
	return left.every((value, index) => value === right[index]);
}

export function EndpointDetailsPage() {
	const { endpointId } = useParams({ from: "/app/endpoints/$endpointId" });
	const navigate = useNavigate();
	const queryClient = useQueryClient();
	const { pushToast } = useToast();
	const adminToken = readAdminToken();

	const endpointQuery = useQuery({
		queryKey: ["adminEndpoint", adminToken, endpointId],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminEndpoint(adminToken, endpointId, signal),
	});

	const [port, setPort] = useState("");
	const [publicDomain, setPublicDomain] = useState("");
	const [realityDest, setRealityDest] = useState("");
	const [realityServerNames, setRealityServerNames] = useState("");
	const [realityFingerprint, setRealityFingerprint] = useState("");
	const [confirmDeleteOpen, setConfirmDeleteOpen] = useState(false);

	useEffect(() => {
		const endpoint = endpointQuery.data;
		if (!endpoint) return;

		setPort(String(endpoint.port));
		if (endpoint.kind === "vless_reality_vision_tcp") {
			const metaSnapshot = parseVlessMeta(endpoint.meta);
			setPublicDomain(metaSnapshot.publicDomain);
			setRealityDest(metaSnapshot.realityDest);
			setRealityServerNames(metaSnapshot.realityServerNames.join(", "));
			setRealityFingerprint(metaSnapshot.realityFingerprint);
		} else {
			setPublicDomain("");
			setRealityDest("");
			setRealityServerNames("");
			setRealityFingerprint("");
		}
	}, [endpointQuery.data]);

	const patchMutation = useMutation({
		mutationFn: async () => {
			const endpoint = endpointQuery.data;
			if (!endpoint) {
				throw new Error("Endpoint is not loaded yet.");
			}
			const portNumber = Number.parseInt(port, 10);
			if (!Number.isFinite(portNumber) || portNumber <= 0) {
				throw new Error("Please enter a valid port.");
			}

			const payload: {
				port?: number;
				public_domain?: string;
				reality?: {
					dest: string;
					server_names: string[];
					fingerprint: string;
				};
			} = { port: portNumber };

			if (endpoint.kind === "vless_reality_vision_tcp") {
				const metaSnapshot = parseVlessMeta(endpoint.meta);
				const publicDomainTrimmed = publicDomain.trim();
				const destTrimmed = realityDest.trim();
				const fingerprintTrimmed = realityFingerprint.trim();
				const serverNames = parseServerNames(realityServerNames);
				const realityChanged =
					destTrimmed !== metaSnapshot.realityDest ||
					fingerprintTrimmed !== metaSnapshot.realityFingerprint ||
					!arraysEqual(serverNames, metaSnapshot.realityServerNames);

				if (publicDomainTrimmed !== metaSnapshot.publicDomain) {
					if (!publicDomainTrimmed) {
						throw new Error("Public domain is required for VLESS endpoints.");
					}
					payload.public_domain = publicDomainTrimmed;
				}

				if (realityChanged) {
					if (!destTrimmed) {
						throw new Error("Reality destination is required.");
					}
					if (serverNames.length === 0) {
						throw new Error("Provide at least one reality server name.");
					}
					if (!fingerprintTrimmed) {
						throw new Error("Reality fingerprint is required.");
					}
					payload.reality = {
						dest: destTrimmed,
						server_names: serverNames,
						fingerprint: fingerprintTrimmed,
					};
				}
			}

			return patchAdminEndpoint(adminToken, endpointId, payload);
		},
		onSuccess: (endpoint) => {
			pushToast({
				variant: "success",
				message: "Endpoint updated successfully.",
			});
			queryClient.setQueryData(
				["adminEndpoint", adminToken, endpointId],
				endpoint,
			);
		},
		onError: (error) => {
			pushToast({
				variant: "error",
				message: formatErrorMessage(error),
			});
		},
	});

	const rotateMutation = useMutation({
		mutationFn: () => rotateAdminEndpointShortId(adminToken, endpointId),
		onSuccess: (data) => {
			pushToast({
				variant: "success",
				message: `shortId rotated: ${data.active_short_id}`,
			});
			endpointQuery.refetch();
		},
		onError: (error) => {
			pushToast({
				variant: "error",
				message: formatErrorMessage(error),
			});
		},
	});

	const deleteMutation = useMutation({
		mutationFn: () => deleteAdminEndpoint(adminToken, endpointId),
		onSuccess: () => {
			pushToast({
				variant: "success",
				message: "Endpoint deleted.",
			});
			navigate({ to: "/endpoints" });
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
				description="Set an admin token to load endpoint details."
				action={
					<Link className="btn btn-primary" to="/login">
						Go to login
					</Link>
				}
			/>
		);
	}

	if (endpointQuery.isLoading) {
		return (
			<PageState
				variant="loading"
				title="Loading endpoint"
				description="Fetching endpoint details from the control plane."
			/>
		);
	}

	if (endpointQuery.isError) {
		const description = formatErrorMessage(endpointQuery.error);
		return (
			<PageState
				variant="error"
				title="Failed to load endpoint"
				description={description}
				action={
					<Button variant="secondary" onClick={() => endpointQuery.refetch()}>
						Retry
					</Button>
				}
			/>
		);
	}

	const endpoint = endpointQuery.data;

	if (!endpoint) {
		return (
			<PageState
				variant="empty"
				title="Endpoint not found"
				description="The requested endpoint does not exist."
				action={
					<Link className="btn btn-primary" to="/endpoints">
						Back to endpoints
					</Link>
				}
			/>
		);
	}

	const vlessMeta =
		endpoint.kind === "vless_reality_vision_tcp"
			? parseVlessMeta(endpoint.meta)
			: null;

	return (
		<div className="space-y-6">
			<div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
				<div>
					<h1 className="text-2xl font-bold">Endpoint details</h1>
					<p className="text-sm opacity-70">Endpoint {endpoint.endpoint_id}</p>
				</div>
				<div className="flex gap-2">
					<Link className="btn btn-ghost" to="/endpoints">
						Back
					</Link>
					<Button
						variant="secondary"
						loading={endpointQuery.isFetching}
						onClick={() => endpointQuery.refetch()}
					>
						Refresh
					</Button>
				</div>
			</div>

			<div className="grid gap-6 lg:grid-cols-2">
				<div className="card bg-base-100 shadow">
					<div className="card-body">
						<h2 className="card-title">Overview</h2>
						<div className="space-y-2 text-sm">
							<p>
								Kind: <span className="font-mono">{endpoint.kind}</span>
							</p>
							<p>
								Node ID: <span className="font-mono">{endpoint.node_id}</span>
							</p>
							<p>
								Port: <span className="font-mono">{endpoint.port}</span>
							</p>
							<p>
								Tag: <span className="font-mono">{endpoint.tag}</span>
							</p>
							<p>
								Endpoint ID:{" "}
								<span className="font-mono">{endpoint.endpoint_id}</span>
							</p>
						</div>
					</div>
				</div>

				<div className="card bg-base-100 shadow">
					<div className="card-body">
						<h2 className="card-title">Configuration</h2>
						{endpoint.kind === "vless_reality_vision_tcp" && vlessMeta ? (
							<div className="space-y-2 text-sm">
								<p>
									Public domain:{" "}
									<span className="font-mono">{vlessMeta.publicDomain}</span>
								</p>
								<p>
									Reality dest:{" "}
									<span className="font-mono">{vlessMeta.realityDest}</span>
								</p>
								<p>
									Server names:{" "}
									<span className="font-mono">
										{vlessMeta.realityServerNames.join(", ") || "-"}
									</span>
								</p>
								<p>
									Fingerprint:{" "}
									<span className="font-mono">
										{vlessMeta.realityFingerprint}
									</span>
								</p>
							</div>
						) : (
							<p className="text-sm opacity-70">
								SS2022 endpoints do not have VLESS Reality configuration.
							</p>
						)}
					</div>
				</div>
			</div>

			<div className="card bg-base-100 shadow">
				<div className="card-body space-y-4">
					<h2 className="card-title">Update endpoint</h2>
					<form
						className="space-y-4"
						onSubmit={(event) => {
							event.preventDefault();
							patchMutation.mutate();
						}}
					>
						<div className="grid gap-4 md:grid-cols-2">
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

						{endpoint.kind === "vless_reality_vision_tcp" ? (
							<div className="space-y-4 border-t border-base-200 pt-4">
								<h3 className="text-lg font-semibold">VLESS settings</h3>
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
						) : (
							<p className="text-sm opacity-70">
								SS2022 endpoints only support port updates.
							</p>
						)}

						<div className="card-actions justify-end">
							<Button loading={patchMutation.isPending} type="submit">
								Save changes
							</Button>
						</div>
					</form>
				</div>
			</div>

			{endpoint.kind === "vless_reality_vision_tcp" ? (
				<div className="card bg-base-100 shadow">
					<div className="card-body space-y-4">
						<h2 className="card-title">Rotate shortId</h2>
						<p className="text-sm opacity-70">
							Generate a new shortId for this VLESS endpoint.
						</p>
						<div>
							<Button
								variant="secondary"
								loading={rotateMutation.isPending}
								onClick={() => rotateMutation.mutate()}
							>
								Rotate shortId
							</Button>
						</div>
					</div>
				</div>
			) : null}

			<div className="card bg-base-100 shadow border border-error/30">
				<div className="card-body space-y-4">
					<h2 className="card-title text-error">Danger zone</h2>
					<p className="text-sm opacity-70">
						Deleting an endpoint will remove it from the cluster configuration.
					</p>
					<button
						type="button"
						className="btn btn-error"
						onClick={() => setConfirmDeleteOpen(true)}
						disabled={deleteMutation.isPending}
					>
						Delete endpoint
					</button>
				</div>
			</div>

			<ConfirmDialog
				open={confirmDeleteOpen}
				title="Delete endpoint"
				description="This action cannot be undone. Are you sure you want to delete this endpoint?"
				confirmLabel={deleteMutation.isPending ? "Deleting..." : "Delete"}
				onCancel={() => setConfirmDeleteOpen(false)}
				onConfirm={() => {
					setConfirmDeleteOpen(false);
					deleteMutation.mutate();
				}}
			/>
		</div>
	);
}
