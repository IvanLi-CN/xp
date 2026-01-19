import { useQuery } from "@tanstack/react-query";
import { Link, useParams } from "@tanstack/react-router";
import { useEffect, useMemo, useState } from "react";

import {
	type AdminNodePatchRequest,
	fetchAdminNode,
	patchAdminNode,
} from "../api/adminNodes";
import { isBackendApiError } from "../api/backendError";
import { Button } from "../components/Button";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
import { useToast } from "../components/Toast";
import { useUiPrefs } from "../components/UiPrefs";
import { readAdminToken } from "../components/auth";

function formatErrorMessage(error: unknown): string {
	if (isBackendApiError(error)) {
		const code = error.code ? ` ${error.code}` : "";
		return `${error.status}${code}: ${error.message}`;
	}
	return String(error);
}

export function NodeDetailsPage() {
	const { nodeId } = useParams({ from: "/app/nodes/$nodeId" });
	const [adminToken] = useState(() => readAdminToken());
	const { pushToast } = useToast();
	const prefs = useUiPrefs();

	const nodeQuery = useQuery({
		queryKey: ["adminNode", adminToken, nodeId],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminNode(adminToken, nodeId, signal),
	});

	const [nodeName, setNodeName] = useState("");
	const [accessHost, setAccessHost] = useState("");
	const [apiBaseUrl, setApiBaseUrl] = useState("");
	const [saveError, setSaveError] = useState<string | null>(null);
	const [isSaving, setIsSaving] = useState(false);

	useEffect(() => {
		if (nodeQuery.data) {
			setNodeName(nodeQuery.data.node_name);
			setAccessHost(nodeQuery.data.access_host);
			setApiBaseUrl(nodeQuery.data.api_base_url);
		}
	}, [nodeQuery.data]);

	const isDirty = useMemo(() => {
		if (!nodeQuery.data) return false;
		return (
			nodeName !== nodeQuery.data.node_name ||
			accessHost !== nodeQuery.data.access_host ||
			apiBaseUrl !== nodeQuery.data.api_base_url
		);
	}, [accessHost, apiBaseUrl, nodeName, nodeQuery.data]);

	const handleSave = async () => {
		if (!nodeQuery.data) return;
		if (!isDirty) {
			pushToast({ variant: "info", message: "No changes to save." });
			return;
		}

		setIsSaving(true);
		setSaveError(null);

		const payload: AdminNodePatchRequest = {};
		if (nodeName !== nodeQuery.data.node_name) {
			payload.node_name = nodeName;
		}
		if (accessHost !== nodeQuery.data.access_host) {
			payload.access_host = accessHost;
		}
		if (apiBaseUrl !== nodeQuery.data.api_base_url) {
			payload.api_base_url = apiBaseUrl;
		}

		try {
			await patchAdminNode(adminToken, nodeId, payload);
			pushToast({ variant: "success", message: "Node updated." });
			await nodeQuery.refetch();
		} catch (error) {
			const message = formatErrorMessage(error);
			setSaveError(message);
			pushToast({
				variant: "error",
				message: "Failed to update node.",
			});
		} finally {
			setIsSaving(false);
		}
	};

	const content = (() => {
		if (adminToken.length === 0) {
			return (
				<PageState
					variant="empty"
					title="Admin token required"
					description="Please provide an admin token to load node details."
				/>
			);
		}

		if (nodeQuery.isLoading) {
			return (
				<PageState
					variant="loading"
					title="Loading node"
					description="Fetching node metadata."
				/>
			);
		}

		if (nodeQuery.isError) {
			return (
				<PageState
					variant="error"
					title="Failed to load node"
					description={formatErrorMessage(nodeQuery.error)}
					action={
						<Button
							variant="secondary"
							loading={nodeQuery.isFetching}
							onClick={() => nodeQuery.refetch()}
						>
							Retry
						</Button>
					}
				/>
			);
		}

		if (!nodeQuery.data) {
			return (
				<PageState
					variant="empty"
					title="Node not found"
					description="No node data is available for this ID."
				/>
			);
		}

		return (
			<div className="card bg-base-100 shadow">
				<div className="card-body space-y-4">
					<div>
						<h2 className="card-title">Node metadata</h2>
						<p className="text-sm opacity-70">
							Update display and routing attributes for this node.
						</p>
					</div>
					<div className="rounded-box bg-base-200 p-4">
						<p className="text-xs uppercase tracking-wide opacity-60">
							Node ID
						</p>
						<p className="font-mono text-sm break-all">{nodeId}</p>
					</div>
					<div className="grid gap-4 md:grid-cols-2">
						<label className="form-control">
							<div className="label">
								<span className="label-text">Node name</span>
							</div>
							<input
								type="text"
								className={
									prefs.density === "compact"
										? "input input-bordered input-sm"
										: "input input-bordered"
								}
								value={nodeName}
								onChange={(event) => setNodeName(event.target.value)}
								placeholder="e.g. node-1"
							/>
						</label>
						<label className="form-control">
							<div className="label">
								<span className="label-text">Access host</span>
							</div>
							<input
								type="text"
								className={
									prefs.density === "compact"
										? "input input-bordered input-sm font-mono"
										: "input input-bordered font-mono"
								}
								value={accessHost}
								onChange={(event) => setAccessHost(event.target.value)}
								placeholder="example.com"
							/>
						</label>
						<label className="form-control md:col-span-2">
							<div className="label">
								<span className="label-text">API base URL</span>
							</div>
							<input
								type="text"
								className={
									prefs.density === "compact"
										? "input input-bordered input-sm font-mono"
										: "input input-bordered font-mono"
								}
								value={apiBaseUrl}
								onChange={(event) => setApiBaseUrl(event.target.value)}
								placeholder="https://node-1.internal:8443"
							/>
						</label>
					</div>
					{saveError ? (
						<p className="text-sm text-error font-mono">{saveError}</p>
					) : null}
					<div className="card-actions justify-end gap-2">
						<Button
							variant="secondary"
							loading={nodeQuery.isFetching}
							onClick={() => nodeQuery.refetch()}
						>
							Refresh
						</Button>
						<Button loading={isSaving} disabled={!isDirty} onClick={handleSave}>
							Save
						</Button>
					</div>
				</div>
			</div>
		);
	})();

	return (
		<div className="space-y-6">
			<PageHeader
				title="Node details"
				description="Manage node metadata and routing configuration."
				actions={
					<Link to="/nodes" className="btn btn-ghost btn-sm">
						Back
					</Link>
				}
			/>
			{content}
		</div>
	);
}
