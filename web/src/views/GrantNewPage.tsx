import { useQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { useEffect, useState } from "react";

import { fetchAdminEndpoints } from "../api/adminEndpoints";
import { type CyclePolicy, createAdminGrant } from "../api/adminGrants";
import { fetchAdminNodes } from "../api/adminNodes";
import { fetchAdminUserNodeQuotas } from "../api/adminUserNodeQuotas";
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
	const [cyclePolicy, setCyclePolicy] = useState<CyclePolicy>("inherit_user");
	const [cycleDay, setCycleDay] = useState(1);
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

	const nodeQuotasQuery = useQuery({
		queryKey: ["adminUserNodeQuotas", adminToken, userId],
		enabled: adminToken.length > 0 && userId.length > 0,
		queryFn: ({ signal }) =>
			fetchAdminUserNodeQuotas(adminToken, userId, signal),
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
					description="Set an admin token to create grants."
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
			endpointsQuery.isLoading ||
			nodeQuotasQuery.isLoading
		) {
			return (
				<PageState
					variant="loading"
					title="Loading grant form"
					description="Fetching nodes, users, endpoints and node quotas."
				/>
			);
		}

		if (
			nodesQuery.isError ||
			usersQuery.isError ||
			endpointsQuery.isError ||
			nodeQuotasQuery.isError
		) {
			const message = usersQuery.isError
				? formatError(usersQuery.error)
				: nodesQuery.isError
					? formatError(nodesQuery.error)
					: endpointsQuery.isError
						? formatError(endpointsQuery.error)
						: nodeQuotasQuery.isError
							? formatError(nodeQuotasQuery.error)
							: "Unknown error";
			return (
				<PageState
					variant="error"
					title="Failed to load grant form"
					description={message}
					action={
						<Button
							variant="secondary"
							onClick={() => {
								nodesQuery.refetch();
								usersQuery.refetch();
								endpointsQuery.refetch();
								nodeQuotasQuery.refetch();
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
		const nodeQuotas = nodeQuotasQuery.data?.items ?? [];

		if (nodes.length === 0 || users.length === 0 || endpoints.length === 0) {
			return (
				<PageState
					variant="empty"
					title="Missing dependencies"
					description={
						nodes.length === 0
							? "Create a node before creating grants."
							: users.length === 0
								? "Create a user before creating grants."
								: "Create an endpoint before creating grants."
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
					const selectedOne =
						selectedEndpointIds.length === 1 ? selectedEndpointIds[0] : null;
					if (!userId || !selectedOne) {
						setError(
							selectedEndpointIds.length === 0
								? "User and access point are required."
								: selectedEndpointIds.length > 1
									? "Select exactly one access point to create a single grant."
									: "User and access point are required.",
						);
						return;
					}
					if (cyclePolicy !== "inherit_user") {
						if (cycleDay < 1 || cycleDay > 31) {
							setError("Cycle day must be between 1 and 31.");
							return;
						}
					}
					setError(null);
					setIsSubmitting(true);
					try {
						const selectedEndpoint =
							endpoints.find((ep) => ep.endpoint_id === selectedOne) ?? null;
						const quotaLimitBytes = selectedEndpoint
							? (nodeQuotas.find((q) => q.node_id === selectedEndpoint.node_id)
									?.quota_limit_bytes ?? 0)
							: 0;

						const payload = {
							user_id: userId,
							endpoint_id: selectedOne,
							quota_limit_bytes: quotaLimitBytes,
							cycle_policy: cyclePolicy,
							cycle_day_of_month:
								cyclePolicy === "inherit_user" ? null : cycleDay,
							note: note.trim() ? note.trim() : null,
						};
						const created = await createAdminGrant(adminToken, payload);
						pushToast({
							variant: "success",
							message: "Grant created.",
						});
						navigate({
							to: "/grants/$grantId",
							params: { grantId: created.grant_id },
						});
					} catch (err) {
						setError(formatError(err));
						pushToast({
							variant: "error",
							message: "Failed to create grant.",
						});
					} finally {
						setIsSubmitting(false);
					}
				}}
			>
				<div className="space-y-6">
					<div className="max-w-xl">
						<label className="form-control">
							<div className="label">
								<span className="label-text">User</span>
							</div>
							<select
								className={selectClass}
								value={userId}
								onChange={(event) => setUserId(event.target.value)}
							>
								{users.map((user) => (
									<option key={user.user_id} value={user.user_id}>
										{user.display_name} ({user.user_id})
									</option>
								))}
							</select>
						</label>
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
								disabled={selectedEndpointIds.length === 0}
							>
								Reset
							</Button>
							<Button
								type="submit"
								size="sm"
								loading={isSubmitting}
								disabled={
									isSubmitting ||
									userId.length === 0 ||
									selectedEndpointIds.length !== 1
								}
							>
								Create grant
							</Button>
						</div>

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
								Quota is configured per node in{" "}
								<Link
									to="/users/$userId"
									params={{ userId }}
									className="link link-primary"
								>
									user details
								</Link>
								.
							</p>
						</div>
						<label className="form-control">
							<div className="label">
								<span className="label-text">Cycle policy</span>
							</div>
							<select
								className={selectClass}
								value={cyclePolicy}
								onChange={(event) => {
									const next = event.target.value as CyclePolicy;
									setCyclePolicy(next);
									if (next === "inherit_user") {
										setCycleDay(1);
									}
								}}
							>
								<option value="inherit_user">inherit_user</option>
								<option value="by_user">by_user</option>
								<option value="by_node">by_node</option>
							</select>
						</label>
					</div>
					<label className="form-control">
						<div className="label">
							<span className="label-text">Cycle day of month</span>
						</div>
						<input
							className={inputClass}
							type="number"
							min={1}
							max={31}
							value={cycleDay}
							onChange={(event) => setCycleDay(Number(event.target.value))}
							disabled={cyclePolicy === "inherit_user"}
						/>
						{cyclePolicy === "inherit_user" ? (
							<p className="text-xs opacity-70">
								Cycle day is inherited from the user.
							</p>
						) : null}
					</label>
					<label className="form-control">
						<div className="label">
							<span className="label-text">Note (optional)</span>
						</div>
						<textarea
							className={textareaClass}
							value={note}
							onChange={(event) => setNote(event.target.value)}
							placeholder="e.g. enterprise quota"
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
				title="Access points"
				description={
					selectedUser ? (
						<>
							User ID: <span className="font-mono">{selectedUser.user_id}</span>{" "}
							- {selectedUser.display_name}
						</>
					) : (
						"Select a node and protocol combination to create one grant."
					)
				}
				actions={
					<Link to="/grants" className="btn btn-ghost btn-sm">
						Back
					</Link>
				}
			/>
			{content}
		</div>
	);
}
