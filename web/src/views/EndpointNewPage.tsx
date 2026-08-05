import { zodResolver } from "@hookform/resolvers/zod";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { useEffect } from "react";
import { useForm } from "react-hook-form";
import { z } from "zod";
import { fetchAdminConfig } from "../api/adminConfig";
import {
	createAdminEndpoint,
	fetchAdminEndpoints,
} from "../api/adminEndpoints";
import { fetchAdminNodes } from "../api/adminNodes";
import { isBackendApiError } from "../api/backendError";
import { useApiCapability } from "../api/useApiCompatibility";
import { AutocompleteInput } from "../components/AutocompleteInput";
import { Button } from "../components/Button";
import { PageHeader } from "../components/PageHeader";
import { CapabilityUnavailableState, PageState } from "../components/PageState";
import { TagInput } from "../components/TagInput";
import { useToast } from "../components/Toast";
import { readAdminToken } from "../components/auth";
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
import { validateAcceptedAuthority } from "../utils/acceptedAuthority";
import {
	MANAGED_VLESS_ACCEPTED_HOST_HELPER_TEXT,
	MANAGED_VLESS_MODE_HELPER_TEXT,
	acceptedAuthoritySuggestionsFromAccessHost,
	canaryUpstreamSuggestionsFromAuthorities,
	canaryUpstreamSuggestionsFromManagedEndpointDests,
	mergeManagedVlessAutocompleteSuggestions,
	normalizeAcceptedAuthorities,
} from "../utils/managedVlessForm";

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
	kind: z.enum(["vless_reality_vision_tcp", "ss2022_2022_blake3_aes_128_gcm"]),
	nodeId: z.string().min(1, "Node is required."),
	port: z.coerce.number().int().positive("Please enter a valid port."),
	canaryUpstreamUrl: z.string(),
	canaryUpstreamMode: z.enum(["auto", "http1", "h2c"]),
	acceptedAuthorities: z.array(z.string()),
});

type EndpointFormValues = z.infer<typeof endpointSchema>;
type EndpointFormInput = z.input<typeof endpointSchema>;

export function EndpointNewPage() {
	const navigate = useNavigate();
	const { pushToast } = useToast();
	const adminToken = readAdminToken();
	const endpointsCapability = useApiCapability("admin.endpoints");
	const nodesCapability = useApiCapability("admin.nodes");
	const configCapability = useApiCapability("admin.config");

	const nodesQuery = useQuery({
		queryKey: ["adminNodes", adminToken],
		enabled: adminToken.length > 0 && nodesCapability.available,
		queryFn: ({ signal }) => fetchAdminNodes(adminToken, signal),
	});
	const endpointsQuery = useQuery({
		queryKey: ["adminEndpoints", adminToken],
		enabled: adminToken.length > 0 && endpointsCapability.available,
		queryFn: ({ signal }) => fetchAdminEndpoints(adminToken, signal),
	});
	const adminConfigQuery = useQuery({
		queryKey: ["adminConfig", adminToken],
		enabled: adminToken.length > 0 && configCapability.available,
		queryFn: ({ signal }) => fetchAdminConfig(adminToken, signal),
	});
	const form = useForm<EndpointFormInput, unknown, EndpointFormValues>({
		resolver: zodResolver(endpointSchema),
		defaultValues: {
			kind: "vless_reality_vision_tcp",
			nodeId: "",
			port: 443,
			canaryUpstreamUrl: "",
			canaryUpstreamMode: "auto",
			acceptedAuthorities: [],
		},
	});

	const kind = form.watch("kind");
	const nodeId = form.watch("nodeId");
	const port = form.watch("port") as number | string | undefined;
	const nodes = nodesQuery.data?.items ?? [];
	const selectedNode = nodes.find((node) => node.node_id === nodeId);
	const canaryUpstreamSuggestions = mergeManagedVlessAutocompleteSuggestions([
		...canaryUpstreamSuggestionsFromManagedEndpointDests(
			endpointsQuery.data?.items ?? [],
			selectedNode?.node_id ?? nodeId,
		),
		...canaryUpstreamSuggestionsFromAuthorities([
			adminConfigQuery.data?.vless_https_canary_bind,
		]),
	]);
	const acceptedAuthoritySuggestions =
		acceptedAuthoritySuggestionsFromAccessHost(selectedNode?.access_host, port);

	useEffect(() => {
		if (nodes.length === 0) return;
		if (!nodeId || !nodes.some((node) => node.node_id === nodeId)) {
			form.setValue("nodeId", nodes[0]?.node_id ?? "", { shouldDirty: false });
		}
	}, [form, nodeId, nodes]);

	const createMutation = useMutation({
		mutationFn: async (values: EndpointFormValues) => {
			if (adminToken.length === 0) {
				throw new Error("Missing admin token.");
			}

			if (values.kind === "vless_reality_vision_tcp") {
				const canaryUpstreamUrl = values.canaryUpstreamUrl.trim();
				const acceptedAuthorities = normalizeAcceptedAuthorities(
					values.acceptedAuthorities,
				);

				for (const authority of acceptedAuthorities) {
					const err = validateAcceptedAuthority(authority);
					if (err) throw new Error(err);
				}

				return createAdminEndpoint(adminToken, {
					kind: values.kind,
					node_id: values.nodeId,
					port: values.port,
					canary_upstream: canaryUpstreamUrl
						? {
								url: canaryUpstreamUrl,
								mode: values.canaryUpstreamMode,
							}
						: undefined,
					accepted_authorities:
						acceptedAuthorities.length > 0 ? acceptedAuthorities : undefined,
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
			pushToast({ variant: "error", message: formatErrorMessage(error) });
		},
	});

	if (
		endpointsCapability.unavailable ||
		nodesCapability.unavailable ||
		configCapability.unavailable
	) {
		return (
			<CapabilityUnavailableState
				title="Endpoint creation unavailable"
				reason={
					endpointsCapability.reason ??
					nodesCapability.reason ??
					configCapability.reason
				}
			/>
		);
	}

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
					error={nodesQuery.error}
					action={
						<Button variant="secondary" onClick={() => nodesQuery.refetch()}>
							Retry
						</Button>
					}
				/>
			</div>
		);
	}

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
												onValueChange={field.onChange}
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
														<SelectItem key={node.node_id} value={node.node_id}>
															{node.node_name} ({node.node_id})
														</SelectItem>
													))}
												</SelectContent>
											</Select>
											<FormDescription>
												Endpoints are created on the selected node.
											</FormDescription>
											<FormMessage />
										</FormItem>
									)}
								/>
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
													type="number"
													min={1}
													name={field.name}
													ref={field.ref}
													onBlur={field.onBlur}
													value={
														typeof field.value === "number" ||
														typeof field.value === "string"
															? field.value
															: ""
													}
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
									<div className="space-y-4">
										<div className="grid gap-4 md:grid-cols-[1fr_180px]">
											<FormField
												control={form.control}
												name="canaryUpstreamUrl"
												render={({ field }) => (
													<FormItem>
														<FormLabel className="font-mono">
															canaryUpstreamUrl
														</FormLabel>
														<FormControl>
															<AutocompleteInput
																{...field}
																type="url"
																placeholder="http://127.0.0.1:8080"
																suggestions={canaryUpstreamSuggestions}
																suggestionLabel="Show XP HTTPS listener suggestions"
																onSuggestionSelect={field.onChange}
															/>
														</FormControl>
														<FormDescription>
															Requests other than GET/HEAD /generate_204 are
															proxied to this origin.
														</FormDescription>
														<FormMessage />
													</FormItem>
												)}
											/>
											<FormField
												control={form.control}
												name="canaryUpstreamMode"
												render={({ field }) => (
													<FormItem>
														<FormLabel className="font-mono">mode</FormLabel>
														<Select
															value={field.value}
															onValueChange={field.onChange}
														>
															<FormControl>
																<SelectTrigger aria-label="canary upstream mode">
																	<SelectValue />
																</SelectTrigger>
															</FormControl>
															<SelectContent>
																<SelectItem value="auto">auto</SelectItem>
																<SelectItem value="http1">http1</SelectItem>
																<SelectItem value="h2c">h2c</SelectItem>
															</SelectContent>
														</Select>
														<FormDescription>
															{MANAGED_VLESS_MODE_HELPER_TEXT}
														</FormDescription>
														<FormMessage />
													</FormItem>
												)}
											/>
										</div>
										<FormField
											control={form.control}
											name="acceptedAuthorities"
											render={({ field }) => (
												<FormItem>
													<FormControl>
														<TagInput
															label="accepted host[:port]"
															value={field.value ?? []}
															onChange={(next) =>
																field.onChange(
																	normalizeAcceptedAuthorities(next),
																)
															}
															placeholder="edge.example.com"
															disabled={createMutation.isPending}
															validateTag={validateAcceptedAuthority}
															allowPrimary={false}
															suggestions={acceptedAuthoritySuggestions}
															suggestionLabel="Show access host suggestions"
															helperText={
																MANAGED_VLESS_ACCEPTED_HOST_HELPER_TEXT
															}
														/>
													</FormControl>
													<FormMessage />
												</FormItem>
											)}
										/>
									</div>
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
									disabled={createMutation.isPending}
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
