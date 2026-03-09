import { zodResolver } from "@hookform/resolvers/zod";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { useEffect, useMemo } from "react";
import { useForm } from "react-hook-form";
import { z } from "zod";

import {
	AdminEndpointKindSchema,
	createAdminEndpoint,
} from "../api/adminEndpoints";
import { fetchAdminNodes } from "../api/adminNodes";
import { fetchAdminRealityDomains } from "../api/adminRealityDomains";
import { isBackendApiError } from "../api/backendError";
import { Button } from "../components/Button";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
import { TagInput } from "../components/TagInput";
import { useToast } from "../components/Toast";
import { readAdminToken } from "../components/auth";
import { Badge } from "../components/ui/badge";
import {
	Card,
	CardContent,
	CardHeader,
	CardTitle,
} from "../components/ui/card";
import {
	Form,
	FormControl,
	FormDescription,
	FormField,
	FormItem,
	FormLabel,
	FormMessage,
} from "../components/ui/form";
import { Input } from "../components/ui/input";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "../components/ui/select";
import { deriveGlobalRealityServerNames } from "../utils/realityDomains";
import {
	normalizeRealityServerName,
	validateRealityServerName,
} from "../utils/realityServerName";

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

const endpointSchema = z.object({
	kind: AdminEndpointKindSchema,
	nodeId: z.string().min(1, "Node is required."),
	port: z.coerce.number().int().positive("Please enter a valid port."),
	realityServerNamesSource: z.enum(["manual", "global"]),
	realityServerNamesManual: z.array(z.string()),
	realityFingerprint: z.string(),
});

type EndpointFormValues = z.infer<typeof endpointSchema>;

export function EndpointNewPage() {
	const navigate = useNavigate();
	const { pushToast } = useToast();
	const adminToken = readAdminToken();

	const nodesQuery = useQuery({
		queryKey: ["adminNodes", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminNodes(adminToken, signal),
	});

	const realityDomainsQuery = useQuery({
		queryKey: ["adminRealityDomains", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminRealityDomains(adminToken, signal),
	});

	const form = useForm<EndpointFormValues>({
		resolver: zodResolver(endpointSchema),
		defaultValues: {
			kind: "vless_reality_vision_tcp",
			nodeId: "",
			port: 443,
			realityServerNamesSource: "global",
			realityServerNamesManual: [],
			realityFingerprint: "chrome",
		},
	});

	const kind = form.watch("kind");
	const nodeId = form.watch("nodeId");
	const realityServerNamesSource = form.watch("realityServerNamesSource");

	useEffect(() => {
		const nodes = nodesQuery.data?.items ?? [];
		if (nodes.length === 0) return;
		if (!nodeId || !nodes.some((node) => node.node_id === nodeId)) {
			form.setValue("nodeId", nodes[0]?.node_id ?? "", { shouldDirty: false });
		}
	}, [form, nodeId, nodesQuery.data]);

	const derivedGlobalServerNames = useMemo(() => {
		if (!nodeId) return [];
		const domains = realityDomainsQuery.data?.items ?? [];
		return deriveGlobalRealityServerNames(domains, nodeId);
	}, [nodeId, realityDomainsQuery.data]);

	const globalDomainsReady =
		!realityDomainsQuery.isLoading && !realityDomainsQuery.isError;
	const globalHasDomains = derivedGlobalServerNames.length > 0;

	const createMutation = useMutation({
		mutationFn: async (values: EndpointFormValues) => {
			if (adminToken.length === 0) {
				throw new Error("Missing admin token.");
			}

			if (values.kind === "vless_reality_vision_tcp") {
				const fingerprintValue = values.realityFingerprint.trim() || "chrome";
				const serverNames =
					values.realityServerNamesSource === "global"
						? derivedGlobalServerNames
						: values.realityServerNamesManual
								.map(normalizeRealityServerName)
								.filter((serverName) => serverName.length > 0);

				if (values.realityServerNamesSource === "manual") {
					if (serverNames.length === 0) {
						throw new Error("serverName is required.");
					}
					for (const name of serverNames) {
						const err = validateRealityServerName(name);
						if (err) throw new Error(err);
					}
				} else {
					if (realityDomainsQuery.isLoading) {
						throw new Error("Reality domains are still loading.");
					}
					if (realityDomainsQuery.isError) {
						throw new Error("Failed to load reality domains.");
					}
					if (serverNames.length === 0) {
						throw new Error(
							"No enabled reality domains for this node. Add some in Settings > Reality domains.",
						);
					}
				}

				const primary = serverNames[0];
				return createAdminEndpoint(adminToken, {
					kind: values.kind,
					node_id: values.nodeId,
					port: values.port,
					reality: {
						dest: `${primary}:443`,
						server_names: serverNames,
						server_names_source: values.realityServerNamesSource,
						fingerprint: fingerprintValue,
					},
				});
			}

			return createAdminEndpoint(adminToken, {
				kind: values.kind,
				node_id: values.nodeId,
				port: values.port,
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
			<div className="space-y-6">
				<PageHeader
					title="New endpoint"
					description="Create an ingress endpoint for a node."
					actions={
						<Button asChild variant="ghost" size="sm">
							<Link to="/endpoints">Back</Link>
						</Button>
					}
				/>
				<PageState
					variant="empty"
					title="Admin token required"
					description="Set an admin token to create endpoints."
					action={
						<Button asChild>
							<Link to="/login">Go to login</Link>
						</Button>
					}
				/>
			</div>
		);
	}

	if (nodesQuery.isLoading) {
		return (
			<div className="space-y-6">
				<PageHeader
					title="New endpoint"
					description="Create an ingress endpoint for a node."
					actions={
						<Button asChild variant="ghost" size="sm">
							<Link to="/endpoints">Back</Link>
						</Button>
					}
				/>
				<PageState
					variant="loading"
					title="Loading nodes"
					description="Fetching nodes for endpoint assignment."
				/>
			</div>
		);
	}

	if (nodesQuery.isError) {
		return (
			<div className="space-y-6">
				<PageHeader
					title="New endpoint"
					description="Create an ingress endpoint for a node."
					actions={
						<Button asChild variant="ghost" size="sm">
							<Link to="/endpoints">Back</Link>
						</Button>
					}
				/>
				<PageState
					variant="error"
					title="Failed to load nodes"
					description={formatErrorMessage(nodesQuery.error)}
					action={
						<Button variant="secondary" onClick={() => nodesQuery.refetch()}>
							Retry
						</Button>
					}
				/>
			</div>
		);
	}

	const nodes = nodesQuery.data?.items ?? [];
	if (nodes.length === 0) {
		return (
			<div className="space-y-6">
				<PageHeader
					title="New endpoint"
					description="Create an ingress endpoint for a node."
					actions={
						<Button asChild variant="ghost" size="sm">
							<Link to="/endpoints">Back</Link>
						</Button>
					}
				/>
				<PageState
					variant="empty"
					title="No nodes available"
					description="Create or register a node before adding endpoints."
					action={
						<Button asChild>
							<Link to="/nodes">Go to nodes</Link>
						</Button>
					}
				/>
			</div>
		);
	}

	return (
		<div className="space-y-6">
			<PageHeader
				title="New endpoint"
				description="Create an ingress endpoint for a node."
				actions={
					<Button asChild variant="ghost" size="sm">
						<Link to="/endpoints">Back</Link>
					</Button>
				}
			/>
			<Card>
				<CardHeader>
					<CardTitle>Create endpoint</CardTitle>
				</CardHeader>
				<CardContent>
					<Form {...form}>
						<form
							className="space-y-6"
							onSubmit={form.handleSubmit(async (values) => {
								try {
									form.clearErrors("root");
									await createMutation.mutateAsync(values);
								} catch (error) {
									form.setError("root", { message: formatErrorMessage(error) });
								}
							})}
						>
							<div className="grid gap-4 md:grid-cols-2">
								<FormField
									control={form.control}
									name="kind"
									render={({ field }) => (
										<FormItem>
											<FormLabel>Kind</FormLabel>
											<Select
												value={field.value}
												onValueChange={(value) =>
													field.onChange(AdminEndpointKindSchema.parse(value))
												}
											>
												<FormControl>
													<SelectTrigger>
														<SelectValue />
													</SelectTrigger>
												</FormControl>
												<SelectContent>
													{kindOptions.map((option) => (
														<SelectItem key={option.value} value={option.value}>
															{option.label}
														</SelectItem>
													))}
												</SelectContent>
											</Select>
											<FormMessage />
										</FormItem>
									)}
								/>

								{nodes.length <= 1 ? (
									<div className="space-y-2">
										<p className="text-sm font-medium">Node</p>
										<div className="rounded-xl border border-border/70 bg-muted/35 px-3 py-3 text-sm">
											<span className="font-medium">
												{nodes[0]?.node_name ?? "Node"}
											</span>{" "}
											<span className="font-mono text-xs text-muted-foreground">
												({nodes[0]?.node_id ?? nodeId})
											</span>
										</div>
									</div>
								) : (
									<FormField
										control={form.control}
										name="nodeId"
										render={({ field }) => (
											<FormItem>
												<FormLabel>Node</FormLabel>
												<Select
													value={field.value}
													onValueChange={field.onChange}
												>
													<FormControl>
														<SelectTrigger>
															<SelectValue placeholder="Choose a node" />
														</SelectTrigger>
													</FormControl>
													<SelectContent>
														{nodes.map((node) => (
															<SelectItem
																key={node.node_id}
																value={node.node_id}
															>
																{node.node_name} ({node.node_id})
															</SelectItem>
														))}
													</SelectContent>
												</Select>
												<FormMessage />
											</FormItem>
										)}
									/>
								)}
							</div>

							<div className="space-y-4 border-t border-border/70 pt-6">
								<h2 className="text-lg font-semibold">
									{kind === "vless_reality_vision_tcp"
										? "VLESS settings"
										: "SS2022 settings"}
								</h2>

								<FormField
									control={form.control}
									name="port"
									render={({ field }) => (
										<FormItem>
											<FormLabel className="font-mono">port</FormLabel>
											<FormControl>
												<Input
													{...field}
													type="number"
													min={1}
													onChange={(event) =>
														field.onChange(event.target.value)
													}
												/>
											</FormControl>
											<FormDescription>
												The inbound listen port on this node.
											</FormDescription>
											<FormMessage />
										</FormItem>
									)}
								/>

								{kind === "vless_reality_vision_tcp" ? (
									<>
										<FormField
											control={form.control}
											name="realityServerNamesSource"
											render={({ field }) => (
												<FormItem>
													<FormLabel className="font-mono">
														serverNamesSource
													</FormLabel>
													<Select
														value={field.value}
														onValueChange={field.onChange}
														disabled={createMutation.isPending}
													>
														<FormControl>
															<SelectTrigger>
																<SelectValue />
															</SelectTrigger>
														</FormControl>
														<SelectContent>
															<SelectItem value="global">global</SelectItem>
															<SelectItem value="manual">manual</SelectItem>
														</SelectContent>
													</Select>
													<FormDescription>
														<span className="font-mono">global</span> derives
														serverNames from{" "}
														<Link className="xp-link" to="/reality-domains">
															Settings &gt; Reality domains
														</Link>{" "}
														(enabled per node).{" "}
														<span className="font-mono">manual</span> stores the
														list on this endpoint.
													</FormDescription>
													<FormMessage />
												</FormItem>
											)}
										/>

										{realityServerNamesSource === "manual" ? (
											<FormField
												control={form.control}
												name="realityServerNamesManual"
												render={({ field }) => (
													<FormItem>
														<FormControl>
															<TagInput
																label="serverNames"
																value={field.value ?? []}
																onChange={field.onChange}
																placeholder="download.example.com"
																disabled={createMutation.isPending}
																validateTag={validateRealityServerName}
																helperText="Camouflage domains (TLS SNI). First tag is primary (used for dest/probe). Subscription may randomly output one of the tags."
															/>
														</FormControl>
														<FormMessage />
													</FormItem>
												)}
											/>
										) : (
											<div className="space-y-2">
												<p className="font-mono text-sm font-medium">
													derived serverNames
												</p>
												<div className="rounded-xl border border-border/70 bg-muted/35 px-3 py-3 text-sm">
													{realityDomainsQuery.isLoading ? (
														<span className="text-muted-foreground">
															Loading reality domains...
														</span>
													) : realityDomainsQuery.isError ? (
														<span className="text-destructive">
															Failed to load reality domains.
														</span>
													) : derivedGlobalServerNames.length === 0 ? (
														<span className="text-warning-foreground">
															No enabled domains for this node.
														</span>
													) : (
														<div className="flex flex-wrap gap-2">
															{derivedGlobalServerNames.map((name, index) => (
																<Badge
																	key={`${index}:${name}`}
																	variant={index === 0 ? "default" : "ghost"}
																	className="gap-2 font-mono"
																>
																	<span>{name}</span>
																	{index === 0 ? (
																		<span className="opacity-80">primary</span>
																	) : null}
																</Badge>
															))}
														</div>
													)}
												</div>
												<p className="text-xs text-muted-foreground">
													Derived from the ordered registry; the first enabled
													domain becomes primary.
												</p>
											</div>
										)}

										<details className="rounded-2xl border border-border/70 bg-muted/35 px-4 py-3">
											<summary className="cursor-pointer text-sm font-medium">
												Advanced (optional)
											</summary>
											<div className="mt-4">
												<FormField
													control={form.control}
													name="realityFingerprint"
													render={({ field }) => (
														<FormItem>
															<FormLabel className="font-mono">
																fingerprint
															</FormLabel>
															<FormControl>
																<Input
																	{...field}
																	type="text"
																	placeholder="chrome"
																/>
															</FormControl>
															<FormDescription>
																Defaults to{" "}
																<span className="font-mono">chrome</span>.
															</FormDescription>
															<FormMessage />
														</FormItem>
													)}
												/>
											</div>
										</details>
									</>
								) : null}
							</div>

							{form.formState.errors.root?.message ? (
								<p className="text-sm font-medium text-destructive">
									{form.formState.errors.root.message}
								</p>
							) : null}

							<div className="flex flex-wrap justify-end gap-2">
								<Button asChild variant="ghost">
									<Link to="/endpoints">Cancel</Link>
								</Button>
								<Button
									type="submit"
									loading={createMutation.isPending}
									disabled={
										createMutation.isPending ||
										(kind === "vless_reality_vision_tcp" &&
											realityServerNamesSource === "global" &&
											(!globalDomainsReady || !globalHasDomains))
									}
								>
									Create endpoint
								</Button>
							</div>
						</form>
					</Form>
				</CardContent>
			</Card>
		</div>
	);
}
