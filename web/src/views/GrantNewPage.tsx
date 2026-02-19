import { useQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { useEffect, useState } from "react";

import type { AdminEndpoint } from "../api/adminEndpoints";
import { fetchAdminEndpoints } from "../api/adminEndpoints";
import {
	type AdminGrantGroupCreateRequest,
	createAdminGrantGroup,
} from "../api/adminGrantGroups";
import { fetchAdminNodes } from "../api/adminNodes";
import { fetchAdminUsers } from "../api/adminUsers";
import { isBackendApiError } from "../api/backendError";
import { Button } from "../components/Button";
import {
	GrantAccessMatrix,
	type GrantAccessMatrixCellState,
} from "../components/GrantAccessMatrix";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
import { useToast } from "../components/Toast";
import { useUiPrefs } from "../components/UiPrefs";
import { readAdminToken } from "../components/auth";

function formatError(err: unknown): string {
	if (isBackendApiError(err)) {
		const code = err.code ? ` ${err.code}` : "";
		return `${err.status}${code}: ${err.message}`;
	}
	if (err instanceof Error) return err.message;
	return String(err);
}

function generateDefaultGroupName(): string {
	const date = new Date();
	const yyyymmdd = [
		date.getFullYear(),
		String(date.getMonth() + 1).padStart(2, "0"),
		String(date.getDate()).padStart(2, "0"),
	].join("");

	let rand = "";
	if (typeof crypto !== "undefined" && "getRandomValues" in crypto) {
		const bytes = new Uint8Array(4);
		crypto.getRandomValues(bytes);
		rand = Array.from(bytes)
			.map((b) => b.toString(16).padStart(2, "0"))
			.join("");
	} else {
		rand = Math.random().toString(16).slice(2, 10);
	}

	return `group-${yyyymmdd}-${rand}`.slice(0, 64);
}

function validateGroupNameInput(raw: string): string | null {
	const name = raw.trim();
	if (name.length === 0) return "Group name is required.";
	if (name.length > 64) return "Group name must be 64 characters or fewer.";
	if (!/^[a-z0-9][a-z0-9-_]*$/.test(name)) {
		return "Group name must match: [a-z0-9][a-z0-9-_]*";
	}
	return null;
}

export function buildGrantGroupCreateRequest(args: {
	groupName: string;
	userId: string;
	selectedEndpointIds: string[];
	endpoints: AdminEndpoint[];
	note: string;
}): AdminGrantGroupCreateRequest {
	const groupName = args.groupName.trim();
	const noteValue = args.note.trim() ? args.note.trim() : null;

	const members = args.selectedEndpointIds.map((endpointId) => {
		const endpoint = args.endpoints.find((ep) => ep.endpoint_id === endpointId);
		if (!endpoint) {
			throw new Error(`endpoint not found: ${endpointId}`);
		}
		return {
			user_id: args.userId,
			endpoint_id: endpointId,
			enabled: true,
			// Shared node quota policy does not use static per-member quotas.
			quota_limit_bytes: 0,
			note: noteValue,
		};
	});

	return {
		group_name: groupName,
		members,
	};
}

export function GrantNewPage() {
	const adminToken = readAdminToken();
	const navigate = useNavigate();
	const { pushToast } = useToast();
	const prefs = useUiPrefs();

	const inputClass =
		prefs.density === "compact"
			? "input input-bordered input-sm"
			: "input input-bordered";
	const selectClass =
		prefs.density === "compact"
			? "select select-bordered select-sm"
			: "select select-bordered";
	const textareaClass =
		prefs.density === "compact"
			? "textarea textarea-bordered textarea-sm"
			: "textarea textarea-bordered";

	const [userId, setUserId] = useState("");
	const [nodeFilter, setNodeFilter] = useState("");
	const [selectedByCell, setSelectedByCell] = useState<Record<string, string>>(
		{},
	);
	const [groupName, setGroupName] = useState(generateDefaultGroupName);
	const [groupNameTouched, setGroupNameTouched] = useState(false);
	const [note, setNote] = useState("");
	const [error, setError] = useState<string | null>(null);
	const [isSubmitting, setIsSubmitting] = useState(false);

	const nodesQuery = useQuery({
		queryKey: ["adminNodes", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminNodes(adminToken, signal),
	});

	const usersQuery = useQuery({
		queryKey: ["adminUsers", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminUsers(adminToken, signal),
	});

	const endpointsQuery = useQuery({
		queryKey: ["adminEndpoints", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminEndpoints(adminToken, signal),
	});

	const selectedUser =
		usersQuery.data?.items.find((u) => u.user_id === userId) ?? null;

	useEffect(() => {
		if (!usersQuery.data) return;
		if (!userId && usersQuery.data.items.length > 0) {
			setUserId(usersQuery.data.items[0].user_id);
		}
	}, [userId, usersQuery.data]);

	const content = (() => {
		if (adminToken.length === 0) {
			return (
				<PageState
					variant="empty"
					title="Admin token required"
					description="Set an admin token to create grant groups."
					action={
						<Link to="/login" className="btn btn-primary">
							Go to login
						</Link>
					}
				/>
			);
		}

		if (
			nodesQuery.isLoading ||
			usersQuery.isLoading ||
			endpointsQuery.isLoading
		) {
			return (
				<PageState
					variant="loading"
					title="Loading grant group form"
					description="Fetching nodes, users, and endpoints."
				/>
			);
		}

		if (nodesQuery.isError || usersQuery.isError || endpointsQuery.isError) {
			const message = usersQuery.isError
				? formatError(usersQuery.error)
				: nodesQuery.isError
					? formatError(nodesQuery.error)
					: endpointsQuery.isError
						? formatError(endpointsQuery.error)
						: "Unknown error";
			return (
				<PageState
					variant="error"
					title="Failed to load grant group form"
					description={message}
					action={
						<Button
							variant="secondary"
							onClick={() => {
								nodesQuery.refetch();
								usersQuery.refetch();
								endpointsQuery.refetch();
							}}
						>
							Retry
						</Button>
					}
				/>
			);
		}

		const nodes = nodesQuery.data?.items ?? [];
		const users = usersQuery.data?.items ?? [];
		const endpoints = endpointsQuery.data?.items ?? [];

		if (nodes.length === 0 || users.length === 0 || endpoints.length === 0) {
			return (
				<PageState
					variant="empty"
					title="Missing dependencies"
					description={
						nodes.length === 0
							? "Create a node before creating grant groups."
							: users.length === 0
								? "Create a user before creating grant groups."
								: "Create an endpoint before creating grant groups."
					}
				/>
			);
		}

		const PROTOCOLS = [
			{ protocolId: "vless_reality_vision_tcp", label: "VLESS" },
			{ protocolId: "ss2022_2022_blake3_aes_128_gcm", label: "SS2022" },
		] as const;

		const cellKey = (nodeId: string, protocolId: string) =>
			`${nodeId}::${protocolId}`;

		const endpointsByNodeProtocol = (() => {
			const map = new Map<string, Map<string, typeof endpoints>>();
			for (const ep of endpoints) {
				const protocolId = ep.kind;
				const supported = PROTOCOLS.some((p) => p.protocolId === protocolId);
				if (!supported) continue;
				if (!map.has(ep.node_id)) map.set(ep.node_id, new Map());
				const byProtocol = map.get(ep.node_id);
				if (!byProtocol) continue;
				if (!byProtocol.has(protocolId)) byProtocol.set(protocolId, []);
				byProtocol.get(protocolId)?.push(ep);
			}
			// Stable ordering for deterministic UI.
			for (const [, byProtocol] of map) {
				for (const [, list] of byProtocol) {
					list.sort((a, b) => a.port - b.port || a.tag.localeCompare(b.tag));
				}
			}
			return map;
		})();

		const visibleNodes = nodes.filter((n) => {
			const q = nodeFilter.trim().toLowerCase();
			if (!q) return true;
			return (
				n.node_name.toLowerCase().includes(q) ||
				n.node_id.toLowerCase().includes(q)
			);
		});

		const selectedEndpointIds = Object.values(selectedByCell);
		const totalEndpointOptions = endpoints.filter((ep) =>
			PROTOCOLS.some((p) => p.protocolId === ep.kind),
		).length;

		const groupNameError = validateGroupNameInput(groupName);

		const submitDisabled =
			isSubmitting ||
			userId.length === 0 ||
			selectedEndpointIds.length === 0 ||
			groupNameError !== null;

		const submitLabel =
			selectedEndpointIds.length <= 1
				? "Create group"
				: `Create group (${selectedEndpointIds.length} members)`;

		const cells: Record<
			string,
			Record<string, GrantAccessMatrixCellState>
		> = {};
		for (const n of visibleNodes) {
			const row: Record<string, GrantAccessMatrixCellState> = {};
			for (const p of PROTOCOLS) {
				const options =
					endpointsByNodeProtocol.get(n.node_id)?.get(p.protocolId) ?? [];
				if (options.length === 0) {
					row[p.protocolId] = { value: "disabled", reason: "No endpoint" };
					continue;
				}

				const key = cellKey(n.node_id, p.protocolId);
				const selected = selectedByCell[key];
				const selectedEp = selected
					? (options.find((ep) => ep.endpoint_id === selected) ?? null)
					: null;
				row[p.protocolId] = {
					value: selectedEp ? "on" : "off",
					meta:
						options.length > 1
							? {
									options: options.map((ep) => ({
										endpointId: ep.endpoint_id,
										tag: ep.tag,
										port: ep.port,
									})),
									selectedEndpointId: selectedEp?.endpoint_id,
									port: selectedEp?.port,
								}
							: {
									endpointId: options[0].endpoint_id,
									tag: options[0].tag,
									port: options[0].port,
								},
				};
			}
			cells[n.node_id] = row;
		}

		const onToggleCell = (nodeId: string, protocolId: string) => {
			const options =
				endpointsByNodeProtocol.get(nodeId)?.get(protocolId) ?? [];
			if (options.length === 0) return;
			const key = cellKey(nodeId, protocolId);
			setSelectedByCell((prev) => {
				const next = { ...prev };
				if (next[key]) delete next[key];
				else next[key] = options[0].endpoint_id;
				return next;
			});
		};

		const onSelectCellEndpoint = (
			nodeId: string,
			protocolId: string,
			endpointId: string,
		) => {
			const options =
				endpointsByNodeProtocol.get(nodeId)?.get(protocolId) ?? [];
			if (!options.some((ep) => ep.endpoint_id === endpointId)) return;
			const key = cellKey(nodeId, protocolId);
			setSelectedByCell((prev) => ({ ...prev, [key]: endpointId }));
		};

		const onToggleRow = (nodeId: string) => {
			const protocolIds = PROTOCOLS.map((p) => p.protocolId);
			setSelectedByCell((prev) => {
				const hasAny = protocolIds.some((pid) =>
					Boolean(prev[cellKey(nodeId, pid)]),
				);
				const next = { ...prev };
				for (const pid of protocolIds) {
					const key = cellKey(nodeId, pid);
					const options = endpointsByNodeProtocol.get(nodeId)?.get(pid) ?? [];
					if (options.length === 0) continue;
					if (hasAny) delete next[key];
					else next[key] = options[0].endpoint_id;
				}
				return next;
			});
		};

		const onToggleColumn = (protocolId: string) => {
			setSelectedByCell((prev) => {
				const hasAny = visibleNodes.some((n) =>
					Boolean(prev[cellKey(n.node_id, protocolId)]),
				);
				const next = { ...prev };
				for (const n of visibleNodes) {
					const key = cellKey(n.node_id, protocolId);
					const options =
						endpointsByNodeProtocol.get(n.node_id)?.get(protocolId) ?? [];
					if (options.length === 0) continue;
					if (hasAny) delete next[key];
					else next[key] = options[0].endpoint_id;
				}
				return next;
			});
		};

		const onToggleAll = () => {
			setSelectedByCell((prev) => {
				const hasAny = Object.keys(prev).length > 0;
				if (hasAny) return {};
				const next: Record<string, string> = {};
				for (const n of visibleNodes) {
					for (const p of PROTOCOLS) {
						const key = cellKey(n.node_id, p.protocolId);
						const options =
							endpointsByNodeProtocol.get(n.node_id)?.get(p.protocolId) ?? [];
						if (options.length === 0) continue;
						next[key] = options[0].endpoint_id;
					}
				}
				return next;
			});
		};

		return (
			<form
				className="rounded-box border border-base-200 bg-base-100 p-6 space-y-6"
				onSubmit={async (event) => {
					event.preventDefault();
					if (!userId) {
						setError("User is required.");
						return;
					}
					if (selectedEndpointIds.length === 0) {
						setError("Select at least 1 access point.");
						return;
					}
					if (groupNameError) {
						setError(groupNameError);
						return;
					}
					setError(null);
					setIsSubmitting(true);
					try {
						const payload = buildGrantGroupCreateRequest({
							groupName,
							userId,
							selectedEndpointIds,
							endpoints,
							note,
						});
						const created = await createAdminGrantGroup(adminToken, payload);
						pushToast({
							variant: "success",
							message: `Created group with ${payload.members.length} members.`,
						});
						navigate({
							to: "/grant-groups/$groupName",
							params: { groupName: created.group.group_name },
						});
					} catch (err) {
						const message = formatError(err);
						setError(message);
						pushToast({
							variant: "error",
							message: `Failed to create group: ${message}`,
						});
					} finally {
						setIsSubmitting(false);
					}
				}}
			>
				<div className="space-y-6">
					<div className="max-w-xl">
						<div className="grid gap-4 md:grid-cols-2">
							<label className="form-control">
								<div className="label">
									<span className="label-text">User</span>
								</div>
								<select
									className={selectClass}
									value={userId}
									onChange={(event) => setUserId(event.target.value)}
									disabled={isSubmitting}
								>
									{users.map((user) => (
										<option key={user.user_id} value={user.user_id}>
											{user.display_name} ({user.user_id})
										</option>
									))}
								</select>
							</label>
							<label className="form-control">
								<div className="label">
									<span className="label-text">Group name</span>
								</div>
								<input
									className={inputClass}
									value={groupName}
									disabled={isSubmitting}
									onChange={(event) => {
										setGroupNameTouched(true);
										setGroupName(event.target.value);
									}}
								/>
								{groupNameError ? (
									<p className="text-xs text-error">{groupNameError}</p>
								) : !groupNameTouched ? (
									<p className="text-xs opacity-70">
										A default name is generated, but you can edit it.
									</p>
								) : null}
							</label>
						</div>
					</div>

					<div className="rounded-box border border-base-200 bg-base-100 p-4 space-y-4">
						<div className="flex flex-col gap-3 md:flex-row md:items-center">
							<input
								className={[
									inputClass,
									"w-full md:max-w-sm bg-base-200/30",
								].join(" ")}
								placeholder="Filter nodes..."
								value={nodeFilter}
								onChange={(event) => setNodeFilter(event.target.value)}
							/>

							<div className="flex items-center gap-2">
								<span className="rounded-full border border-base-200 bg-base-200/40 px-4 py-2 font-mono text-xs">
									Selected {selectedEndpointIds.length} / {totalEndpointOptions}
								</span>
							</div>

							<div className="flex-1" />

							<Button
								variant="secondary"
								size="sm"
								onClick={() => setSelectedByCell({})}
								disabled={isSubmitting || selectedEndpointIds.length === 0}
							>
								Reset
							</Button>
							<Button
								type="submit"
								size="sm"
								loading={isSubmitting}
								disabled={submitDisabled}
							>
								{submitLabel}
							</Button>
						</div>

						{selectedEndpointIds.length === 0 ? (
							<p className="text-xs text-warning">
								Select at least 1 access point to create a group.
							</p>
						) : null}

						<div className="flex items-baseline gap-4">
							<span className="text-sm font-semibold">Matrix</span>
							<span className="text-xs opacity-60">
								Batch rule: if any selected, clear; else select all (no invert)
							</span>
						</div>

						<GrantAccessMatrix
							nodes={visibleNodes.map((n) => ({
								nodeId: n.node_id,
								label: n.node_name,
							}))}
							protocols={PROTOCOLS.map((p) => ({
								protocolId: p.protocolId,
								label: p.label,
							}))}
							cells={cells}
							onToggleCell={onToggleCell}
							onToggleRow={onToggleRow}
							onToggleColumn={onToggleColumn}
							onToggleAll={onToggleAll}
							onSelectCellEndpoint={onSelectCellEndpoint}
						/>

						<p className="text-xs opacity-60">
							Tip: header checkboxes can show indeterminate state, but clicking
							never inverts.
						</p>
					</div>

					<div className="grid gap-4 md:grid-cols-2">
						<div className="form-control">
							<div className="label">
								<span className="label-text">Quota</span>
							</div>
							<p className="text-sm opacity-70">
								Quota is shared per node and configured in{" "}
								<Link to="/quota-policy" className="link link-primary">
									Quota policy
								</Link>
								.
							</p>
						</div>
					</div>
					<label className="form-control">
						<div className="label">
							<span className="label-text">Note (optional)</span>
						</div>
						<textarea
							className={textareaClass}
							value={note}
							onChange={(event) => setNote(event.target.value)}
							placeholder="e.g. enterprise quota"
							disabled={isSubmitting}
						/>
					</label>
					{error ? <p className="text-sm text-error">{error}</p> : null}
				</div>
			</form>
		);
	})();

	return (
		<div className="space-y-6">
			<PageHeader
				title="Create grant group"
				description={
					selectedUser ? (
						<>
							User ID: <span className="font-mono">{selectedUser.user_id}</span>{" "}
							- {selectedUser.display_name}
						</>
					) : (
						"Select access points to create a grant group."
					)
				}
				actions={
					<Link to="/grant-groups" className="btn btn-ghost btn-sm">
						Back
					</Link>
				}
			/>
			{content}
		</div>
	);
}
