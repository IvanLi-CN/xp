import { useQuery } from "@tanstack/react-query";
import { Link, useNavigate, useParams } from "@tanstack/react-router";
import { useEffect, useMemo, useState } from "react";

import { fetchAdminEndpoints } from "../api/adminEndpoints";
import {
	type AdminGrantGroupMember,
	deleteAdminGrantGroup,
	fetchAdminGrantGroup,
	replaceAdminGrantGroup,
} from "../api/adminGrantGroups";
import { fetchAdminUsers } from "../api/adminUsers";
import { isBackendApiError } from "../api/backendError";
import { Button } from "../components/Button";
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

export function GrantGroupDetailsPage() {
	const adminToken = readAdminToken();
	const navigate = useNavigate();
	const { pushToast } = useToast();
	const prefs = useUiPrefs();
	const { groupName } = useParams({ from: "/app/grant-groups/$groupName" });

	const groupQuery = useQuery({
		queryKey: ["adminGrantGroup", adminToken, groupName],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) =>
			fetchAdminGrantGroup(adminToken, groupName, signal),
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

	const [draftGroupName, setDraftGroupName] = useState("");
	const [draftMembers, setDraftMembers] = useState<
		Array<Omit<AdminGrantGroupMember, "credentials">>
	>([]);
	const [saveError, setSaveError] = useState<string | null>(null);
	const [deleteError, setDeleteError] = useState<string | null>(null);
	const [isSaving, setIsSaving] = useState(false);
	const [isDeleting, setIsDeleting] = useState(false);

	const inputClass =
		prefs.density === "compact"
			? "input input-bordered input-sm"
			: "input input-bordered";
	const selectClass =
		prefs.density === "compact"
			? "select select-bordered select-sm"
			: "select select-bordered";

	const detail = groupQuery.data;

	useEffect(() => {
		if (!detail) return;
		setDraftGroupName(detail.group.group_name);
		setDraftMembers(
			detail.members.map((m) => ({
				user_id: m.user_id,
				endpoint_id: m.endpoint_id,
				enabled: m.enabled,
				quota_limit_bytes: m.quota_limit_bytes,
				note: m.note,
			})),
		);
		setSaveError(null);
		setDeleteError(null);
	}, [detail]);

	const isDirty = useMemo(() => {
		if (!detail) return false;
		if (draftGroupName !== detail.group.group_name) return true;
		if (draftMembers.length !== detail.members.length) return true;

		const byKey = new Map<string, Omit<AdminGrantGroupMember, "credentials">>();
		for (const m of draftMembers) {
			byKey.set(`${m.user_id}:${m.endpoint_id}`, m);
		}
		for (const m of detail.members) {
			const key = `${m.user_id}:${m.endpoint_id}`;
			const draft = byKey.get(key);
			if (!draft) return true;
			if (draft.enabled !== m.enabled) return true;
			if (draft.quota_limit_bytes !== m.quota_limit_bytes) return true;
			if ((draft.note ?? null) !== (m.note ?? null)) return true;
		}
		return false;
	}, [detail, draftGroupName, draftMembers]);

	const handleSave = async () => {
		if (!detail) return;
		if (!isDirty) {
			pushToast({ variant: "info", message: "No changes to save." });
			return;
		}
		if (draftGroupName.trim().length === 0) {
			setSaveError("Group name is required.");
			return;
		}
		if (draftMembers.length === 0) {
			setSaveError("Grant group must have at least 1 member.");
			return;
		}
		const seen = new Set<string>();
		for (const m of draftMembers) {
			const key = `${m.user_id}:${m.endpoint_id}`;
			if (seen.has(key)) {
				setSaveError(`Duplicate member: ${key}`);
				return;
			}
			seen.add(key);
		}

		setIsSaving(true);
		setSaveError(null);
		try {
			await replaceAdminGrantGroup(adminToken, groupName, {
				rename_to:
					draftGroupName !== detail.group.group_name
						? draftGroupName
						: undefined,
				members: draftMembers,
			});
			pushToast({ variant: "success", message: "Grant group updated." });

			if (draftGroupName !== groupName) {
				navigate({
					to: "/grant-groups/$groupName",
					params: { groupName: draftGroupName },
				});
				return;
			}

			await groupQuery.refetch();
		} catch (error) {
			setSaveError(formatError(error));
			pushToast({ variant: "error", message: "Failed to update grant group." });
		} finally {
			setIsSaving(false);
		}
	};

	const handleDelete = async () => {
		if (!detail) return;
		setIsDeleting(true);
		setDeleteError(null);
		try {
			await deleteAdminGrantGroup(adminToken, groupName);
			pushToast({ variant: "success", message: "Grant group deleted." });
			navigate({ to: "/grant-groups" });
		} catch (error) {
			setDeleteError(formatError(error));
			pushToast({ variant: "error", message: "Failed to delete grant group." });
		} finally {
			setIsDeleting(false);
		}
	};

	const userOptions = usersQuery.data?.items ?? [];
	const endpointOptions = endpointsQuery.data?.items ?? [];
	const [addUserId, setAddUserId] = useState("");
	const [addEndpointId, setAddEndpointId] = useState("");
	const [addEnabled, setAddEnabled] = useState(true);
	const [addNote, setAddNote] = useState<string>("");

	const handleAddMember = () => {
		const userId = addUserId.trim();
		const endpointId = addEndpointId.trim();
		if (!userId || !endpointId) {
			pushToast({
				variant: "error",
				message: "User and endpoint are required.",
			});
			return;
		}
		const key = `${userId}:${endpointId}`;
		if (draftMembers.some((m) => `${m.user_id}:${m.endpoint_id}` === key)) {
			pushToast({ variant: "error", message: `Member already exists: ${key}` });
			return;
		}
		setDraftMembers((prev) => [
			...prev,
			{
				user_id: userId,
				endpoint_id: endpointId,
				enabled: addEnabled,
				// Deprecated: shared node quota policy does not use static per-member quotas.
				quota_limit_bytes: 0,
				note: addNote.trim().length ? addNote.trim() : null,
			},
		]);
		setAddUserId("");
		setAddEndpointId("");
		setAddEnabled(true);
		setAddNote("");
	};

	if (adminToken.length === 0) {
		return (
			<PageState
				variant="empty"
				title="Admin token required"
				description="Set an admin token to load grant group details."
				action={
					<Link to="/login" className="btn btn-primary">
						Go to login
					</Link>
				}
			/>
		);
	}

	if (groupQuery.isLoading) {
		return (
			<PageState
				variant="loading"
				title="Loading grant group"
				description="Fetching grant group details from the xp API."
			/>
		);
	}

	if (groupQuery.isError) {
		return (
			<PageState
				variant="error"
				title="Failed to load grant group"
				description={formatError(groupQuery.error)}
				action={
					<Button variant="secondary" onClick={() => groupQuery.refetch()}>
						Retry
					</Button>
				}
			/>
		);
	}

	if (!detail) {
		return (
			<PageState
				variant="empty"
				title="Grant group not found"
				description="The group name does not exist."
				action={
					<Link
						to="/grant-groups/new"
						className="btn btn-outline btn-sm xp-btn-outline"
					>
						Back to create
					</Link>
				}
			/>
		);
	}

	return (
		<div className="space-y-6">
			<PageHeader
				title="Grant group"
				description={
					<>
						Group: <span className="font-mono">{detail.group.group_name}</span>{" "}
						— {detail.members.length} member(s)
					</>
				}
				actions={
					<div className="flex items-center gap-2">
						<Link
							to="/grant-groups/new"
							className="btn btn-outline btn-sm xp-btn-outline"
						>
							New group
						</Link>
						<Link to="/grant-groups" className="btn btn-ghost btn-sm">
							Back
						</Link>
					</div>
				}
			/>

			<div className="rounded-box border border-base-200 bg-base-100 p-6 space-y-6">
				<div className="grid gap-4 md:grid-cols-2">
					<label className="form-control">
						<div className="label">
							<span className="label-text">Group name</span>
						</div>
						<input
							className={inputClass}
							value={draftGroupName}
							onChange={(e) => setDraftGroupName(e.target.value)}
						/>
					</label>
				</div>

				<div className="flex flex-col gap-2 md:flex-row md:items-start md:justify-between">
					<div className="space-y-1">
						<h2 className="text-lg font-semibold">Members</h2>
						<p className="text-xs opacity-60">
							Member quotas are deprecated. Configure node budgets and user
							weights in{" "}
							<Link to="/quota-policy" className="link link-primary">
								Quota policy
							</Link>
							.
						</p>
					</div>
					<div className="flex items-center gap-2">
						<Button
							variant="primary"
							loading={isSaving}
							disabled={!isDirty || groupQuery.isFetching}
							onClick={handleSave}
						>
							Save
						</Button>
						<Button
							variant="danger"
							loading={isDeleting}
							disabled={groupQuery.isFetching}
							onClick={handleDelete}
						>
							Delete
						</Button>
					</div>
				</div>

				{saveError ? <p className="text-sm text-error">{saveError}</p> : null}
				{deleteError ? (
					<p className="text-sm text-error">{deleteError}</p>
				) : null}

				<div className="overflow-auto rounded-box border border-base-200">
					<table className="table">
						<thead>
							<tr>
								<th>User</th>
								<th>Endpoint</th>
								<th>Enabled</th>
								<th>Note</th>
								<th />
							</tr>
						</thead>
						<tbody>
							{draftMembers.map((m) => (
								<tr key={`${m.user_id}:${m.endpoint_id}`}>
									<td className="font-mono text-xs">{m.user_id}</td>
									<td className="font-mono text-xs">{m.endpoint_id}</td>
									<td>
										<input
											className="toggle toggle-sm"
											type="checkbox"
											checked={m.enabled}
											onChange={(e) => {
												const enabled = e.target.checked;
												setDraftMembers((prev) =>
													prev.map((row) =>
														row.user_id === m.user_id &&
														row.endpoint_id === m.endpoint_id
															? { ...row, enabled }
															: row,
													),
												);
											}}
										/>
									</td>
									<td>
										<input
											className={inputClass}
											value={m.note ?? ""}
											onChange={(e) => {
												const note = e.target.value;
												setDraftMembers((prev) =>
													prev.map((row) =>
														row.user_id === m.user_id &&
														row.endpoint_id === m.endpoint_id
															? { ...row, note: note.trim() ? note : null }
															: row,
													),
												);
											}}
										/>
									</td>
									<td className="text-right">
										<Button
											variant="secondary"
											onClick={() => {
												setDraftMembers((prev) =>
													prev.filter(
														(row) =>
															!(
																row.user_id === m.user_id &&
																row.endpoint_id === m.endpoint_id
															),
													),
												);
											}}
										>
											Remove
										</Button>
									</td>
								</tr>
							))}
						</tbody>
					</table>
				</div>

				<div className="rounded-box border border-base-200 bg-base-50 p-4 space-y-3">
					<h3 className="font-semibold">Add member</h3>
					<div className="grid gap-3 md:grid-cols-2">
						<label className="form-control">
							<div className="label">
								<span className="label-text">User</span>
							</div>
							<select
								className={selectClass}
								value={addUserId}
								onChange={(e) => setAddUserId(e.target.value)}
							>
								<option value="">Select a user…</option>
								{userOptions.map((u) => (
									<option key={u.user_id} value={u.user_id}>
										{u.display_name} ({u.user_id})
									</option>
								))}
							</select>
						</label>
						<label className="form-control">
							<div className="label">
								<span className="label-text">Endpoint</span>
							</div>
							<select
								className={selectClass}
								value={addEndpointId}
								onChange={(e) => setAddEndpointId(e.target.value)}
							>
								<option value="">Select an endpoint…</option>
								{endpointOptions.map((ep) => (
									<option key={ep.endpoint_id} value={ep.endpoint_id}>
										{ep.tag} ({ep.endpoint_id})
									</option>
								))}
							</select>
						</label>
						<label className="form-control">
							<div className="label">
								<span className="label-text">Enabled</span>
							</div>
							<select
								className={selectClass}
								value={addEnabled ? "yes" : "no"}
								onChange={(e) => setAddEnabled(e.target.value === "yes")}
							>
								<option value="yes">yes</option>
								<option value="no">no</option>
							</select>
						</label>
						<label className="form-control md:col-span-2">
							<div className="label">
								<span className="label-text">Note</span>
							</div>
							<input
								className={inputClass}
								value={addNote}
								onChange={(e) => setAddNote(e.target.value)}
								placeholder="Optional"
							/>
						</label>
					</div>
					<div className="flex justify-end">
						<Button onClick={handleAddMember}>Add</Button>
					</div>
				</div>
			</div>
		</div>
	);
}
