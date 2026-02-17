import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate, useParams } from "@tanstack/react-router";
import { useEffect, useMemo, useState } from "react";

import { runAdminEndpointProbeRun } from "../api/adminEndpointProbes";
import {
	deleteAdminEndpoint,
	fetchAdminEndpoint,
	patchAdminEndpoint,
	rotateAdminEndpointShortId,
} from "../api/adminEndpoints";
import { fetchAdminRealityDomains } from "../api/adminRealityDomains";
import { isBackendApiError } from "../api/backendError";
import { Button } from "../components/Button";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
import { TagInput } from "../components/TagInput";
import { useToast } from "../components/Toast";
import { useUiPrefs } from "../components/UiPrefs";
import { readAdminToken } from "../components/auth";
import { deriveGlobalRealityServerNames } from "../utils/realityDomains";
import {
	normalizeRealityServerName,
	validateRealityServerName,
} from "../utils/realityServerName";

type VlessMetaSnapshot = {
	realityDest: string;
	realityServerNames: string[];
	realityServerNamesSource: "manual" | "global";
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

function asRealityServerNamesSource(
	value: unknown,
): "manual" | "global" | undefined {
	if (value === "manual" || value === "global") return value;
	return undefined;
}

function parseVlessMeta(meta: Record<string, unknown>): VlessMetaSnapshot {
	const reality = isRecord(meta.reality) ? meta.reality : undefined;
	const realityDest = asString(reality?.dest) ?? "";
	const realityServerNames = asStringArray(reality?.server_names) ?? [];
	const realityServerNamesSource =
		asRealityServerNamesSource(reality?.server_names_source) ?? "manual";
	const realityFingerprint = asString(reality?.fingerprint) ?? "";

	return {
		realityDest,
		realityServerNames,
		realityServerNamesSource,
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
	const prefs = useUiPrefs();

	const inputClass =
		prefs.density === "compact"
			? "input input-bordered input-sm"
			: "input input-bordered";

	const endpointQuery = useQuery({
		queryKey: ["adminEndpoint", adminToken, endpointId],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminEndpoint(adminToken, endpointId, signal),
	});

	const realityDomainsQuery = useQuery({
		queryKey: ["adminRealityDomains", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminRealityDomains(adminToken, signal),
	});

	const [port, setPort] = useState("");
	const [realityServerNamesSource, setRealityServerNamesSource] = useState<
		"manual" | "global"
	>("manual");
	const [realityServerNamesManual, setRealityServerNamesManual] = useState<
		string[]
	>([]);
	const [realityFingerprint, setRealityFingerprint] = useState("");
	const [confirmRotateOpen, setConfirmRotateOpen] = useState(false);
	const [confirmDeleteOpen, setConfirmDeleteOpen] = useState(false);

	useEffect(() => {
		const endpoint = endpointQuery.data;
		if (!endpoint) return;

		setPort(String(endpoint.port));
		if (endpoint.kind === "vless_reality_vision_tcp") {
			const metaSnapshot = parseVlessMeta(endpoint.meta);
			setRealityServerNamesSource(metaSnapshot.realityServerNamesSource);
			setRealityServerNamesManual(metaSnapshot.realityServerNames);
			setRealityFingerprint(metaSnapshot.realityFingerprint);
		} else {
			setRealityServerNamesSource("manual");
			setRealityServerNamesManual([]);
			setRealityFingerprint("");
		}
	}, [endpointQuery.data]);

	const derivedGlobalServerNames = useMemo(() => {
		const endpoint = endpointQuery.data;
		if (!endpoint) return [];
		if (endpoint.kind !== "vless_reality_vision_tcp") return [];
		const domains = realityDomainsQuery.data?.items ?? [];
		return deriveGlobalRealityServerNames(domains, endpoint.node_id);
	}, [endpointQuery.data, realityDomainsQuery.data]);

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
				reality?: {
					dest: string;
					server_names: string[];
					server_names_source: "manual" | "global";
					fingerprint: string;
				};
			} = { port: portNumber };

			if (endpoint.kind === "vless_reality_vision_tcp") {
				const metaSnapshot = parseVlessMeta(endpoint.meta);
				const fingerprintValue = realityFingerprint.trim() || "chrome";
				const serverNamesSource = realityServerNamesSource;

				const manualServerNames = realityServerNamesManual
					.map(normalizeRealityServerName)
					.filter((s) => s.length > 0);

				const serverNames =
					serverNamesSource === "global"
						? derivedGlobalServerNames.length > 0
							? derivedGlobalServerNames
							: metaSnapshot.realityServerNames
						: manualServerNames;

				const destValue =
					serverNames.length > 0
						? `${serverNames[0]}:443`
						: metaSnapshot.realityDest;

				const realityChanged =
					serverNamesSource !== metaSnapshot.realityServerNamesSource ||
					fingerprintValue !== metaSnapshot.realityFingerprint ||
					(serverNamesSource === "manual" &&
						!arraysEqual(serverNames, metaSnapshot.realityServerNames));

				if (realityChanged) {
					if (serverNamesSource === "manual") {
						if (serverNames.length === 0) {
							throw new Error("serverName is required.");
						}
						for (const name of serverNames) {
							const err = validateRealityServerName(name);
							if (err) throw new Error(err);
						}
					} else if (
						!realityDomainsQuery.isLoading &&
						!realityDomainsQuery.isError &&
						derivedGlobalServerNames.length === 0
					) {
						throw new Error(
							"No enabled reality domains for this node. Add some in Settings > Reality domains.",
						);
					}
					payload.reality = {
						dest: destValue,
						server_names: serverNames,
						server_names_source: serverNamesSource,
						fingerprint: fingerprintValue,
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

	const probeRunMutation = useMutation({
		mutationFn: () => runAdminEndpointProbeRun(adminToken),
		onSuccess: (data) => {
			pushToast({
				variant: "success",
				message: `Probe started (hour=${data.hour}).`,
			});
			queryClient.invalidateQueries({
				queryKey: ["adminEndpoints", adminToken],
			});
			navigate({
				to: "/endpoints/probe/runs/$runId",
				params: { runId: data.run_id },
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
				description="Fetching endpoint details from the xp API."
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

	const effectiveGlobalServerNames = derivedGlobalServerNames.length
		? derivedGlobalServerNames
		: (vlessMeta?.realityServerNames ?? []);

	return (
		<div className="space-y-6">
			<PageHeader
				title="Endpoint details"
				description={`Endpoint ${endpoint.endpoint_id}`}
				actions={
					<div className="flex gap-2">
						<Link className="btn btn-ghost btn-sm" to="/endpoints">
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
				}
			/>

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
								Listen port: <span className="font-mono">{endpoint.port}</span>
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
									<span className="font-mono">serverNamesSource</span>:{" "}
									<span className="font-mono">
										{vlessMeta.realityServerNamesSource}
									</span>
								</p>
								<p>
									<span className="font-mono">serverNames</span>:
								</p>
								<div className="flex flex-wrap gap-2">
									{vlessMeta.realityServerNames.length > 0 ? (
										vlessMeta.realityServerNames.map((name, idx) => (
											<span
												key={`${idx}:${name}`}
												className={[
													"badge font-mono gap-2",
													idx === 0 ? "badge-primary" : "badge-ghost",
												].join(" ")}
												title={
													idx === 0 ? "Primary (used for dest / probe)" : name
												}
											>
												<span>{name}</span>
												{idx === 0 ? (
													<span className="opacity-80">primary</span>
												) : null}
											</span>
										))
									) : (
										<span className="font-mono">-</span>
									)}
								</div>
								<p>
									<span className="font-mono">fingerprint</span>:{" "}
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
						{endpoint.kind === "vless_reality_vision_tcp" ? (
							<div className="space-y-4 border-t border-base-200 pt-4">
								<h3 className="text-lg font-semibold">VLESS settings</h3>
								<div className="grid gap-4">
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
									<label className="form-control">
										<div className="label">
											<span className="label-text font-mono">
												serverNamesSource
											</span>
										</div>
										<select
											className={
												prefs.density === "compact"
													? "select select-bordered select-sm"
													: "select select-bordered"
											}
											value={realityServerNamesSource}
											onChange={(event) =>
												setRealityServerNamesSource(
													event.target.value as "manual" | "global",
												)
											}
											disabled={patchMutation.isPending}
										>
											<option value="global">global</option>
											<option value="manual">manual</option>
										</select>
										<p className="text-xs opacity-70">
											<span className="font-mono">global</span> derives
											serverNames from{" "}
											<Link className="link link-primary" to="/reality-domains">
												Settings &gt; Reality domains
											</Link>
											. <span className="font-mono">manual</span> stores the
											list on this endpoint.
										</p>
									</label>

									{realityServerNamesSource === "manual" ? (
										<TagInput
											label="serverNames"
											value={realityServerNamesManual}
											onChange={setRealityServerNamesManual}
											placeholder="oneclient.sfx.ms"
											disabled={patchMutation.isPending}
											inputClass={inputClass}
											validateTag={validateRealityServerName}
											helperText="Camouflage domains (TLS SNI). First tag is primary (used for dest/probe). Subscription may randomly output one of the tags."
										/>
									) : (
										<div className="form-control">
											<div className="label">
												<span className="label-text font-mono">
													derived serverNames
												</span>
											</div>
											<div className="rounded-lg border border-base-300 bg-base-200 px-3 py-3 text-sm">
												{realityDomainsQuery.isLoading ? (
													<span className="opacity-70">
														Loading reality domains...
													</span>
												) : realityDomainsQuery.isError ? (
													<span className="text-error">
														Failed to load reality domains.
													</span>
												) : effectiveGlobalServerNames.length === 0 ? (
													<span className="text-warning">
														No enabled domains for this node.
													</span>
												) : (
													<div className="flex flex-wrap gap-2">
														{effectiveGlobalServerNames.map((name, idx) => (
															<span
																key={`${idx}:${name}`}
																className={[
																	"badge font-mono gap-2",
																	idx === 0 ? "badge-primary" : "badge-ghost",
																].join(" ")}
																title={
																	idx === 0
																		? "Primary (used for dest / probe)"
																		: name
																}
															>
																<span>{name}</span>
																{idx === 0 ? (
																	<span className="opacity-80">primary</span>
																) : null}
															</span>
														))}
													</div>
												)}
											</div>
											<p className="text-xs opacity-70">
												Derived from the ordered registry; the first enabled
												domain becomes primary.
											</p>
										</div>
									)}
									<details className="collapse collapse-arrow border border-base-200 bg-base-200/40">
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
								<h3 className="text-lg font-semibold">SS2022 settings</h3>
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

						<div className="card-actions justify-end">
							<Button loading={patchMutation.isPending} type="submit">
								Save changes
							</Button>
						</div>
					</form>
				</div>
			</div>

			<div className="card bg-base-100 shadow">
				<div className="card-body space-y-4">
					<h2 className="card-title">Probe</h2>
					<p className="text-sm opacity-70">
						Run a cluster-wide probe for all endpoints. Results are stored
						hourly and shown in the endpoint list.
					</p>
					<div className="card-actions justify-end">
						<Button
							variant="secondary"
							loading={probeRunMutation.isPending}
							onClick={() => probeRunMutation.mutate()}
						>
							Test now
						</Button>
					</div>
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
								onClick={() => setConfirmRotateOpen(true)}
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
				open={confirmRotateOpen}
				title="Rotate shortId"
				description="This will generate a new shortId for this VLESS endpoint. Existing client configs may stop working until clients refresh. Continue?"
				confirmLabel={
					rotateMutation.isPending ? "Rotating..." : "Rotate shortId"
				}
				onCancel={() => setConfirmRotateOpen(false)}
				onConfirm={() => {
					if (rotateMutation.isPending) return;
					setConfirmRotateOpen(false);
					rotateMutation.mutate();
				}}
			/>
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
