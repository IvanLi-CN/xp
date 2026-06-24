import { zodResolver } from "@hookform/resolvers/zod";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { useEffect } from "react";
import { useForm } from "react-hook-form";
import { z } from "zod";

import { createAdminEndpoint } from "../api/adminEndpoints";
import { fetchAdminNodes } from "../api/adminNodes";
import { isBackendApiError } from "../api/backendError";
import { Button } from "../components/Button";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
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

const kindOptions = [
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
	kind: z.literal("ss2022_2022_blake3_aes_128_gcm"),
	nodeId: z.string().min(1, "Node is required."),
	port: z.coerce.number().int().positive("Please enter a valid port."),
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

	const form = useForm<EndpointFormValues>({
		resolver: zodResolver(endpointSchema),
		defaultValues: {
			kind: "ss2022_2022_blake3_aes_128_gcm",
			nodeId: "",
			port: 443,
		},
	});

	const nodeId = form.watch("nodeId");

	useEffect(() => {
		const nodes = nodesQuery.data?.items ?? [];
		if (nodes.length === 0) return;
		if (!nodeId || !nodes.some((node) => node.node_id === nodeId)) {
			form.setValue("nodeId", nodes[0]?.node_id ?? "", { shouldDirty: false });
		}
	}, [form, nodeId, nodesQuery.data]);

	const createMutation = useMutation({
		mutationFn: async (values: EndpointFormValues) => {
			if (adminToken.length === 0) {
				throw new Error("Missing admin token.");
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
								<h2 className="text-lg font-semibold">SS2022 settings</h2>

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
