import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate, useParams } from "@tanstack/react-router";
import { useEffect, useState } from "react";

import {
	type AdminEndpointCanaryProbeResponse,
	runAdminEndpointCanaryProbe,
	runAdminEndpointProbeRun,
} from "../api/adminEndpointProbes";
import {
	DEFAULT_MIHOMO_SMUX_CONFIG,
	type MihomoSmuxConfig,
	deleteAdminEndpoint,
	fetchAdminEndpoint,
	parseMihomoSmuxConfig,
	patchAdminEndpoint,
	rotateAdminEndpointShortId,
} from "../api/adminEndpoints";
import { fetchAdminNodes } from "../api/adminNodes";
import { isBackendApiError } from "../api/backendError";
import { useApiCapability } from "../api/useApiCompatibility";
import { AutocompleteInput } from "../components/AutocompleteInput";
import { Button } from "../components/Button";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { EndpointMihomoSmuxSettings } from "../components/EndpointMihomoSmuxSettings";
import { PageHeader } from "../components/PageHeader";
import { CapabilityUnavailableState, PageState } from "../components/PageState";
import { ReadStateBanner } from "../components/ReadStateBanner";
import { TagInput } from "../components/TagInput";
import { useToast } from "../components/Toast";
import { useUiPrefs } from "../components/UiPrefs";
import { readAdminToken } from "../components/auth";
import {
	badgeClass,
	inputClass as inputControlClass,
	selectClass as selectControlClass,
} from "../components/ui-helpers";
import { Badge } from "../components/ui/badge";
import { Input } from "../components/ui/input";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "../components/ui/select";
import { useAppRuntime } from "../offline/appRuntime";
import {
	formatSyncTimestamp,
	hasQueryData,
	latestQueryDataUpdatedAt,
	queryIsOfflineBlocked,
} from "../offline/queryReadState";
import {
	normalizeAcceptedAuthority,
	validateAcceptedAuthority,
} from "../utils/acceptedAuthority";
import {
	type CanaryUpstreamMode,
	arraysEqual,
	authoritySetsEqual,
	dedupeAuthorities,
	parseVlessMeta,
	routeAuthority,
} from "../utils/endpointDetailsVless";
import {
	acceptedAuthoritySuggestionsFromAccessHost,
	canaryUpstreamSuggestionsFromAuthorities,
} from "../utils/managedVlessForm";
import { changedMihomoSmuxConfig } from "../utils/mihomoSmux";
import {
	normalizeRealityServerName,
	realityServerNameSuggestionFromDest,
	validateRealityServerName,
} from "../utils/realityServerName";

function formatErrorMessage(error: unknown): string {
	if (isBackendApiError(error)) {
		const code = error.code ? ` ${error.code}` : "";
		return `${error.status}${code}: ${error.message}`;
	}
	if (error instanceof Error) return error.message;
	return String(error);
}

export function EndpointDetailsPage() {
	const { endpointId } = useParams({ from: "/app/endpoints/$endpointId" });
	const navigate = useNavigate();
	const queryClient = useQueryClient();
	const { pushToast } = useToast();
	const adminToken = readAdminToken();
	const runtime = useAppRuntime();
	const prefs = useUiPrefs();
	const endpointsCapability = useApiCapability("admin.endpoints");
	const nodesCapability = useApiCapability("admin.nodes");
	const probesCapability = useApiCapability("admin.node-probes");
	const mihomoSmuxCapability = useApiCapability("admin.endpoint-mihomo-smux");

	const inputClass = inputControlClass(prefs.density);
	const selectClass = selectControlClass(prefs.density);
	const endpointQuery = useQuery({
		queryKey: ["adminEndpoint", adminToken, endpointId],
		enabled: adminToken.length > 0 && endpointsCapability.available,
		queryFn: ({ signal }) => fetchAdminEndpoint(adminToken, endpointId, signal),
	});
	const nodesQuery = useQuery({
		queryKey: ["adminNodes", adminToken],
		enabled: adminToken.length > 0 && nodesCapability.available,
		queryFn: ({ signal }) => fetchAdminNodes(adminToken, signal),
	});

	const [port, setPort] = useState("");
	const [realityDest, setRealityDest] = useState("");
	const [realityServerNamesManual, setRealityServerNamesManual] = useState<
		string[]
	>([]);
	const [realityFingerprint, setRealityFingerprint] = useState("");
	const [upstreamUrl, setUpstreamUrl] = useState("");
	const [upstreamMode, setUpstreamMode] = useState<CanaryUpstreamMode>("auto");
	const [acceptedAuthorities, setAcceptedAuthorities] = useState<string[]>([]);
	const [mihomoSmux, setMihomoSmux] = useState<MihomoSmuxConfig>(
		DEFAULT_MIHOMO_SMUX_CONFIG,
	);
	const [mihomoSmuxMaxConnections, setMihomoSmuxMaxConnections] = useState("4");
	const [mihomoSmuxMinStreams, setMihomoSmuxMinStreams] = useState("4");
	const [canaryProbeResult, setCanaryProbeResult] = useState<{
		endpointId: string;
		result: AdminEndpointCanaryProbeResponse;
	} | null>(null);
	const [confirmRotateOpen, setConfirmRotateOpen] = useState(false);
	const [confirmDeleteOpen, setConfirmDeleteOpen] = useState(false);

	useEffect(() => {
		const endpoint = endpointQuery.data;
		if (!endpoint) return;
		setPort(String(endpoint.port));
		const smux = parseMihomoSmuxConfig(endpoint.meta.mihomo_smux);
		setMihomoSmux(smux);
		setMihomoSmuxMaxConnections(String(smux.max_connections));
		setMihomoSmuxMinStreams(String(smux.min_streams));
		if (endpoint.kind === "vless_reality_vision_tcp") {
			const metaSnapshot = parseVlessMeta(endpoint.meta);
			setRealityDest(metaSnapshot.realityDest);
			setRealityServerNamesManual(metaSnapshot.realityServerNames);
			setRealityFingerprint(metaSnapshot.realityFingerprint);
			setUpstreamUrl(metaSnapshot.canaryUpstreamUrl);
			setUpstreamMode(metaSnapshot.canaryUpstreamMode);
			setAcceptedAuthorities(metaSnapshot.acceptedAuthorities);
		} else {
			setRealityDest("");
			setRealityServerNamesManual([]);
			setRealityFingerprint("");
			setUpstreamUrl("");
			setUpstreamMode("auto");
			setAcceptedAuthorities([]);
		}
	}, [endpointQuery.data]);

	const patchMutation = useMutation({
		mutationFn: async () => {
			const endpoint = endpointQuery.data;
			if (!endpoint) throw new Error("Endpoint is not loaded yet.");
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
				canary_upstream?: { url: string; mode: CanaryUpstreamMode } | null;
				accepted_authorities?: string[] | null;
				mihomo_smux?: MihomoSmuxConfig;
			} = { port: portNumber };
			if (endpoint.kind === "ss2022_2022_blake3_aes_128_gcm") {
				const changedMihomoSmux = changedMihomoSmuxConfig(
					mihomoSmuxCapability.available,
					endpoint.meta.mihomo_smux,
					mihomoSmux,
					mihomoSmuxMaxConnections,
					mihomoSmuxMinStreams,
				);
				if (changedMihomoSmux) payload.mihomo_smux = changedMihomoSmux;
			}

			if (endpoint.kind === "vless_reality_vision_tcp") {
				const metaSnapshot = parseVlessMeta(endpoint.meta);
				if (!metaSnapshot.managedDefault) {
					const fingerprintValue = realityFingerprint.trim() || "chrome";
					const destInput = realityDest.trim();

					const serverNames = realityServerNamesManual
						.map(normalizeRealityServerName)
						.filter((s) => s.length > 0);

					const destValue = destInput;

					const realityChanged =
						metaSnapshot.realityServerNamesSource !== "manual" ||
						destValue !== metaSnapshot.realityDest ||
						fingerprintValue !== metaSnapshot.realityFingerprint ||
						!arraysEqual(serverNames, metaSnapshot.realityServerNames);

					if (realityChanged) {
						if (destValue.length === 0) {
							throw new Error("dest is required.");
						}
						if (serverNames.length === 0) {
							throw new Error("serverName is required.");
						}
						for (const name of serverNames) {
							const err = validateRealityServerName(name);
							if (err) throw new Error(err);
						}
						payload.reality = {
							dest: destValue,
							server_names: serverNames,
							server_names_source: "manual",
							fingerprint: fingerprintValue,
						};
					}
				}

				if (metaSnapshot.managedDefault) {
					const nextUrl = upstreamUrl.trim();
					const nextAcceptedAuthorities = acceptedAuthorities
						.map(normalizeAcceptedAuthority)
						.filter((authority) => authority.length > 0);
					for (const authority of nextAcceptedAuthorities) {
						const err = validateAcceptedAuthority(authority);
						if (err) throw new Error(err);
					}
					const upstreamChanged =
						nextUrl !== metaSnapshot.canaryUpstreamUrl ||
						upstreamMode !== metaSnapshot.canaryUpstreamMode;
					const authoritiesChanged = !authoritySetsEqual(
						nextAcceptedAuthorities,
						metaSnapshot.acceptedAuthorities,
					);
					if (upstreamChanged) {
						payload.canary_upstream = nextUrl
							? { url: nextUrl, mode: upstreamMode }
							: null;
					}
					if (authoritiesChanged) {
						payload.accepted_authorities = nextAcceptedAuthorities;
					}
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
			pushToast({ variant: "error", message: formatErrorMessage(error) });
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
			pushToast({ variant: "error", message: formatErrorMessage(error) });
		},
	});

	const deleteMutation = useMutation({
		mutationFn: () => deleteAdminEndpoint(adminToken, endpointId),
		onSuccess: () => {
			pushToast({ variant: "success", message: "Endpoint deleted." });
			navigate({ to: "/endpoints" });
		},
		onError: (error) => {
			pushToast({ variant: "error", message: formatErrorMessage(error) });
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
			pushToast({ variant: "error", message: formatErrorMessage(error) });
		},
	});

	const canaryProbeMutation = useMutation({
		mutationFn: () => runAdminEndpointCanaryProbe(adminToken, endpointId),
		onSuccess: (data) => {
			setCanaryProbeResult({ endpointId, result: data });
			const okCount = data.nodes.filter((node) => node.ok).length;
			const totalCount = data.nodes.length;
			const allOk = totalCount > 0 && okCount === totalCount;
			const firstError = data.nodes.find((node) => !node.ok)?.error;
			pushToast({
				variant: allOk ? "success" : "error",
				message: allOk
					? `Canary probe returned 204 from ${okCount}/${totalCount} nodes.`
					: `Canary probe passed ${okCount}/${totalCount} nodes: ${firstError ?? "unexpected response"}`,
			});
		},
		onError: (error) => {
			pushToast({ variant: "error", message: formatErrorMessage(error) });
		},
	});

	if (endpointsCapability.unavailable || nodesCapability.unavailable) {
		return (
			<CapabilityUnavailableState
				title="Endpoint details unavailable"
				reason={endpointsCapability.reason ?? nodesCapability.reason}
			/>
		);
	}

	if (adminToken.length === 0) {
		return (
			<PageState
				variant="empty"
				title="Admin token required"
				description="Set an admin token to load endpoint details."
				action={
					<Button asChild>
						<Link to="/login">Go to login</Link>
					</Button>
				}
			/>
		);
	}

	if (endpointQuery.isLoading && !hasQueryData(endpointQuery)) {
		return (
			<PageState
				variant="loading"
				title="Loading endpoint"
				description="Fetching endpoint details from the xp API."
			/>
		);
	}

	if (
		!hasQueryData(endpointQuery) &&
		queryIsOfflineBlocked(endpointQuery, runtime.isOnline)
	) {
		return (
			<PageState
				variant="offline"
				title="Offline cache unavailable"
				description="Open this endpoint once while online to keep its latest configuration available offline."
			/>
		);
	}

	if (endpointQuery.isError && !hasQueryData(endpointQuery)) {
		return (
			<PageState
				variant="error"
				title="Failed to load endpoint"
				description={formatErrorMessage(endpointQuery.error)}
				error={endpointQuery.error}
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
					<Button asChild>
						<Link to="/endpoints">Back to endpoints</Link>
					</Button>
				}
			/>
		);
	}

	const vlessMeta =
		endpoint.kind === "vless_reality_vision_tcp"
			? parseVlessMeta(endpoint.meta)
			: null;
	const endpointNode = nodesQuery.data?.items.find(
		(node) => node.node_id === endpoint.node_id,
	);
	const realityServerNameSuggestion =
		realityServerNameSuggestionFromDest(realityDest);
	const realityServerNameSuggestions = realityServerNameSuggestion
		? [realityServerNameSuggestion]
		: [];
	const managedCanaryUpstreamSuggestions =
		canaryUpstreamSuggestionsFromAuthorities(
			vlessMeta?.managedDefault ? [vlessMeta.realityDest] : [],
		);
	const managedAcceptedAuthoritySuggestions =
		acceptedAuthoritySuggestionsFromAccessHost(endpointNode?.access_host, port);
	const currentCanaryProbeResult =
		canaryProbeResult?.endpointId === endpointId
			? canaryProbeResult.result
			: null;
	const latestSyncedAt = latestQueryDataUpdatedAt([endpointQuery, nodesQuery]);
	const showCachedBanner =
		latestSyncedAt !== null &&
		(hasQueryData(endpointQuery) || hasQueryData(nodesQuery)) &&
		(!runtime.isOnline || endpointQuery.isError || nodesQuery.isError);

	return (
		<div className="space-y-6">
			<PageHeader
				title="Endpoint details"
				description={`Endpoint ${endpoint.endpoint_id}`}
				actions={
					<div className="flex gap-2">
						<Button asChild variant="ghost">
							<Link to="/endpoints">Back</Link>
						</Button>
						<Button
							variant="secondary"
							loading={endpointQuery.isFetching}
							disabled={!runtime.isOnline}
							onClick={() => endpointQuery.refetch()}
						>
							Refresh
						</Button>
					</div>
				}
			/>
			{showCachedBanner ? (
				<ReadStateBanner
					tone={!runtime.isOnline ? "warning" : "info"}
					variant="inline"
					dismissible
					errors={[endpointQuery.error, nodesQuery.error]}
					title={
						!runtime.isOnline
							? "Offline endpoint snapshot"
							: "Showing cached endpoint data"
					}
					description={`Last successful sync: ${formatSyncTimestamp(latestSyncedAt)}.`}
				/>
			) : null}

			<div className="grid gap-6 lg:grid-cols-2">
				<div className="xp-card">
					<div className="xp-card-body">
						<h2 className="xp-card-title">Overview</h2>
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

				<div className="xp-card">
					<div className="xp-card-body">
						<h2 className="xp-card-title">Configuration</h2>
						{vlessMeta ? (
							<div className="space-y-2 text-sm">
								<p>
									<span className="font-mono">effectiveSni</span>:{" "}
									<span className="font-mono">
										{vlessMeta.realityServerNames[0] ?? "-"}
									</span>
								</p>
								<p>
									<span className="font-mono">fallbackTarget</span>:{" "}
									<span className="font-mono">{vlessMeta.realityDest}</span>
								</p>
								<p>
									<span className="font-mono">routeAuthority</span>:{" "}
									<span className="font-mono">
										{routeAuthority(
											vlessMeta.managedDefault
												? endpointNode?.access_host
												: vlessMeta.realityServerNames[0],
											endpoint.port,
										)}
									</span>
								</p>
								<p>
									<span className="font-mono">fingerprint</span>:{" "}
									<span className="font-mono">
										{vlessMeta.realityFingerprint}
									</span>
								</p>
								<p>
									<span className="font-mono">managedDefault</span>:{" "}
									<span className="font-mono">
										{String(vlessMeta.managedDefault)}
									</span>
								</p>
								{vlessMeta.managedDefault ? (
									<div className="space-y-1 pt-1">
										<p>
											<span className="font-mono">acceptedAuthorities</span>:{" "}
											<span className="font-mono">
												{vlessMeta.acceptedAuthorities.length}
											</span>
										</p>
										<div className="flex flex-wrap gap-2">
											{vlessMeta.acceptedAuthorities.length > 0 ? (
												vlessMeta.acceptedAuthorities.map((authority) => (
													<Badge
														key={authority}
														variant="ghost"
														className="font-mono"
													>
														{authority}
													</Badge>
												))
											) : (
												<span className="font-mono">-</span>
											)}
										</div>
									</div>
								) : null}
								<div className="flex flex-wrap gap-2 pt-1">
									{vlessMeta.realityServerNames.length > 0 ? (
										vlessMeta.realityServerNames.map((name, idx) => (
											<span
												key={`${idx}:${name}`}
												className={badgeClass(
													idx === 0 ? "primary" : "ghost",
													"default",
													"font-mono gap-2",
												)}
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
							</div>
						) : (
							<p className="text-sm text-muted-foreground">
								SS2022 endpoints do not have VLESS Reality configuration.
							</p>
						)}
					</div>
				</div>
			</div>

			<div className="xp-card">
				<div className="xp-card-body space-y-4">
					<h2 className="xp-card-title">Update endpoint</h2>
					<form
						className="space-y-4"
						onSubmit={(event) => {
							event.preventDefault();
							patchMutation.mutate();
						}}
					>
						<div className="space-y-4 border-t border-border/70 pt-4">
							<h3 className="text-lg font-semibold">
								{endpoint.kind === "vless_reality_vision_tcp"
									? "VLESS settings"
									: "SS2022 settings"}
							</h3>
							<div className="grid gap-4">
								<div className="xp-field-stack">
									<span className="text-sm font-medium font-mono">port</span>
									<Input
										aria-label="port"
										type="number"
										className={inputClass}
										value={port}
										min={1}
										disabled={patchMutation.isPending || runtime.isReadOnly}
										onChange={(event) => setPort(event.target.value)}
									/>
									<p className="text-xs opacity-70">
										The inbound listen port on this node.
									</p>
								</div>

								{vlessMeta && !vlessMeta.managedDefault ? (
									<>
										<div className="xp-field-stack">
											<div className="flex items-center justify-between gap-2">
												<span className="text-sm font-medium font-mono">
													dest
												</span>
											</div>
											<Input
												aria-label="dest"
												type="text"
												className={inputClass}
												value={realityDest}
												placeholder="origin.example.test:443"
												disabled={patchMutation.isPending || runtime.isReadOnly}
												onChange={(event) => setRealityDest(event.target.value)}
											/>
											<p className="text-xs opacity-70">
												REALITY destination origin. Destination is stored
												separately from the SNI list.
											</p>
										</div>

										<TagInput
											label="serverNames"
											value={realityServerNamesManual}
											onChange={setRealityServerNamesManual}
											placeholder="download.example.com"
											disabled={patchMutation.isPending || runtime.isReadOnly}
											inputClass={inputClass}
											validateTag={validateRealityServerName}
											suggestions={realityServerNameSuggestions}
											suggestionLabel="Show destination suggestion"
											helperText="Camouflage domains (TLS SNI). Use the destination suggestion only when it should also be a serverName."
										/>
										<details className="rounded-xl border border-border/70 bg-muted/35">
											<summary className="cursor-pointer list-none px-4 py-3 text-sm font-medium">
												Advanced (optional)
											</summary>
											<div className="space-y-4 border-t border-border/70 px-4 py-4">
												<div className="xp-field-stack">
													<div className="flex items-center justify-between gap-2">
														<span className="text-sm font-medium font-mono">
															fingerprint
														</span>
													</div>
													<Input
														type="text"
														className={inputClass}
														value={realityFingerprint}
														disabled={
															patchMutation.isPending || runtime.isReadOnly
														}
														placeholder="chrome"
														onChange={(event) =>
															setRealityFingerprint(event.target.value)
														}
													/>
													<p className="text-xs opacity-70">
														Defaults to{" "}
														<span className="font-mono">chrome</span>.
													</p>
												</div>
											</div>
										</details>
									</>
								) : null}

								{vlessMeta?.managedDefault ? (
									<div className="space-y-4">
										<div className="grid gap-4 md:grid-cols-[1fr_180px]">
											<div className="xp-field-stack">
												<span className="text-sm font-medium font-mono">
													canaryUpstreamUrl
												</span>
												<AutocompleteInput
													aria-label="canary upstream url"
													type="url"
													className={inputClass}
													value={upstreamUrl}
													placeholder="http://127.0.0.1:8080"
													disabled={
														patchMutation.isPending || runtime.isReadOnly
													}
													suggestions={managedCanaryUpstreamSuggestions}
													suggestionLabel="Show XP HTTPS listener suggestions"
													onChange={(event) =>
														setUpstreamUrl(event.target.value)
													}
													onSuggestionSelect={setUpstreamUrl}
												/>
												<p className="text-xs opacity-70">
													Requests other than GET/HEAD /generate_204 are proxied
													to this origin.
												</p>
											</div>
											<div className="xp-field-stack">
												<span className="text-sm font-medium font-mono">
													mode
												</span>
												<Select
													value={upstreamMode}
													onValueChange={(value) =>
														setUpstreamMode(value as CanaryUpstreamMode)
													}
													disabled={
														patchMutation.isPending || runtime.isReadOnly
													}
												>
													<SelectTrigger
														className={selectClass}
														aria-label="canary upstream mode"
													>
														<SelectValue />
													</SelectTrigger>
													<SelectContent>
														<SelectItem value="auto">auto</SelectItem>
														<SelectItem value="http1">http1</SelectItem>
														<SelectItem value="h2c">h2c</SelectItem>
													</SelectContent>
												</Select>
												<p className="text-xs opacity-70">
													Use an origin URL only. WebSocket uses HTTP/1.1
													upstream; h2c is for non-upgrade HTTP.
												</p>
											</div>
										</div>
										<TagInput
											label="accepted host[:port]"
											value={acceptedAuthorities}
											onChange={(next) =>
												setAcceptedAuthorities(
													dedupeAuthorities(
														next
															.map(normalizeAcceptedAuthority)
															.filter((authority) => authority.length > 0),
													),
												)
											}
											placeholder="edge.example.com"
											disabled={patchMutation.isPending || runtime.isReadOnly}
											inputClass={inputClass}
											validateTag={validateAcceptedAuthority}
											allowPrimary={false}
											suggestions={managedAcceptedAuthoritySuggestions}
											suggestionLabel="Show access host suggestions"
											helperText="Accept additional ordinary HTTPS Host headers for camouflage routing. Omit port to use HTTPS default 443. This does not change REALITY serverNames or the canonical /generate_204 URL."
										/>
									</div>
								) : null}

								{endpointQuery.data?.kind ===
								"ss2022_2022_blake3_aes_128_gcm" ? (
									<EndpointMihomoSmuxSettings
										config={mihomoSmux}
										available={mihomoSmuxCapability.available}
										disabled={patchMutation.isPending || runtime.isReadOnly}
										inputClass={inputClass}
										maxConnections={mihomoSmuxMaxConnections}
										minStreams={mihomoSmuxMinStreams}
										onConfigChange={setMihomoSmux}
										onMaxConnectionsChange={setMihomoSmuxMaxConnections}
										onMinStreamsChange={setMihomoSmuxMinStreams}
									/>
								) : null}
							</div>
						</div>

						<div className="xp-card-actions justify-end">
							<Button
								loading={patchMutation.isPending}
								type="submit"
								disabled={runtime.isReadOnly}
							>
								Save changes
							</Button>
						</div>
					</form>
				</div>
			</div>

			<div className="xp-card">
				<div className="xp-card-body space-y-4">
					<h2 className="xp-card-title">Probe</h2>
					<div className="grid gap-4">
						<div className="rounded-xl border border-border/70 bg-muted/25 px-4 py-4">
							<div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
								<div className="space-y-1">
									<h3 className="text-sm font-semibold">Proxy path</h3>
									<p className="text-sm text-muted-foreground">
										Run a cluster-wide probe for all endpoints. Results are
										stored hourly and shown in the endpoint list.
									</p>
								</div>
								<Button
									variant="secondary"
									loading={probeRunMutation.isPending}
									disabled={runtime.isReadOnly || probesCapability.unavailable}
									onClick={() => probeRunMutation.mutate()}
								>
									Test now
								</Button>
							</div>
						</div>

						{vlessMeta?.managedDefault ? (
							<div className="rounded-xl border border-border/70 bg-muted/25 px-4 py-4">
								<div className="flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
									<div className="space-y-2">
										<div className="space-y-1">
											<h3 className="text-sm font-semibold">
												Canary /generate_204
											</h3>
											<p className="text-sm text-muted-foreground">
												Test ordinary HTTPS access to the managed VLESS
												fallback. This checks DNS, public ingress, TLS, and xp
												canary without touching upstream.
											</p>
										</div>
										{currentCanaryProbeResult ? (
											<div className="space-y-3 text-xs">
												<p className="break-all font-mono">
													{currentCanaryProbeResult.url}
												</p>
												<div className="flex flex-wrap items-center gap-2">
													{(() => {
														const okCount =
															currentCanaryProbeResult.nodes.filter(
																(node) => node.ok,
															).length;
														const totalCount =
															currentCanaryProbeResult.nodes.length;
														return (
															<Badge
																variant={
																	totalCount > 0 && okCount === totalCount
																		? "default"
																		: "destructive"
																}
															>
																{okCount} / {totalCount} nodes OK
															</Badge>
														);
													})()}
												</div>
												<div className="space-y-2">
													{currentCanaryProbeResult.nodes.map((node) => (
														<div
															key={node.node_id}
															className="flex flex-col gap-1 rounded-lg border border-border/70 bg-background/60 px-3 py-2 sm:flex-row sm:items-center sm:justify-between"
														>
															<div className="flex min-w-0 flex-wrap items-center gap-2">
																<span className="font-mono text-muted-foreground">
																	{node.node_id}
																</span>
																<Badge
																	variant={node.ok ? "default" : "destructive"}
																>
																	{node.ok ? "204 OK" : "failed"}
																</Badge>
																{node.status ? (
																	<span className="font-mono text-muted-foreground">
																		status={node.status}
																	</span>
																) : null}
															</div>
															<div className="flex min-w-0 flex-wrap items-center gap-2 sm:justify-end">
																<span className="text-muted-foreground">
																	{node.latency_ms} ms
																</span>
																{node.error ? (
																	<span className="break-all text-destructive">
																		{node.error}
																	</span>
																) : null}
															</div>
														</div>
													))}
												</div>
												{currentCanaryProbeResult.nodes.length === 0 ? (
													<Badge variant="destructive">no nodes returned</Badge>
												) : null}
											</div>
										) : null}
									</div>
									<Button
										variant="secondary"
										loading={canaryProbeMutation.isPending}
										disabled={
											runtime.isReadOnly || probesCapability.unavailable
										}
										onClick={() => canaryProbeMutation.mutate()}
									>
										Test canary
									</Button>
								</div>
							</div>
						) : null}
					</div>
				</div>
			</div>

			{endpoint.kind === "vless_reality_vision_tcp" ? (
				<div className="xp-card">
					<div className="xp-card-body space-y-4">
						<h2 className="xp-card-title">Rotate shortId</h2>
						<p className="text-sm text-muted-foreground">
							Generate a new shortId for this VLESS endpoint.
						</p>
						<div>
							<Button
								variant="secondary"
								loading={rotateMutation.isPending}
								disabled={runtime.isReadOnly}
								onClick={() => setConfirmRotateOpen(true)}
							>
								Rotate shortId
							</Button>
						</div>
					</div>
				</div>
			) : null}

			<div className="xp-card border border-destructive/30">
				<div className="xp-card-body space-y-4">
					<h2 className="xp-card-title text-destructive">Danger zone</h2>
					<p className="text-sm text-muted-foreground">
						Deleting an endpoint will remove it from the cluster configuration.
					</p>
					<Button
						variant="danger"
						onClick={() => setConfirmDeleteOpen(true)}
						disabled={deleteMutation.isPending || runtime.isReadOnly}
					>
						Delete endpoint
					</Button>
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
