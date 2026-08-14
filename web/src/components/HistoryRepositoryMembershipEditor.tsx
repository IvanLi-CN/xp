import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

import {
	type HistoryRepositoryMember,
	replaceAdminHistoryRepositories,
} from "../api/adminHistoryRepositories";
import { formatBackendError } from "../utils/backendErrorMessage";
import { Button } from "./Button";
import { Checkbox } from "./ui/checkbox";

export function HistoryRepositoryMembershipEditor(props: {
	adminToken: string;
	members: HistoryRepositoryMember[];
	nodes: Array<{ node_id: string; node_name: string }>;
	disabled: boolean;
}) {
	const { adminToken, members, nodes, disabled } = props;
	const queryClient = useQueryClient();
	const [editing, setEditing] = useState(false);
	const [selectedNodeIds, setSelectedNodeIds] = useState<string[]>([]);
	const [error, setError] = useState<string | null>(null);
	const [saving, setSaving] = useState(false);
	const nodeOptions = [...nodes].sort((left, right) =>
		left.node_name.localeCompare(right.node_name),
	);

	const beginEditing = () => {
		setSelectedNodeIds(members.map((member) => member.identity.node_id));
		setError(null);
		setEditing(true);
	};

	const save = async () => {
		if (selectedNodeIds.length === 0) {
			setError("Select at least one cluster node.");
			return;
		}
		setSaving(true);
		setError(null);
		try {
			await replaceAdminHistoryRepositories(adminToken, selectedNodeIds);
			await queryClient.invalidateQueries({
				queryKey: ["adminHistoryRepositories", adminToken],
			});
			setEditing(false);
		} catch (saveError) {
			setError(formatBackendError(saveError));
		} finally {
			setSaving(false);
		}
	};

	if (!editing) {
		return (
			<div className="space-y-2">
				{nodeOptions.length === 0 ? (
					<p className="text-sm text-muted-foreground">
						No cluster nodes are available for repository membership.
					</p>
				) : null}
				<div className="flex flex-wrap justify-end gap-2">
					<Button
						variant="secondary"
						disabled={disabled || nodeOptions.length === 0}
						onClick={beginEditing}
					>
						Edit membership
					</Button>
				</div>
			</div>
		);
	}

	return (
		<section className="space-y-3 border-t border-border/70 pt-4">
			<p className="text-sm font-medium">Repository membership</p>
			<div className="divide-y divide-border rounded-md border border-border">
				{nodeOptions.map((node) => {
					const selected = selectedNodeIds.includes(node.node_id);
					const checkboxId = `history-repository-member-${node.node_id}`;
					return (
						<label
							key={node.node_id}
							htmlFor={checkboxId}
							className="flex min-w-0 cursor-pointer items-center gap-3 px-3 py-2 text-sm"
						>
							<Checkbox
								id={checkboxId}
								checked={selected}
								disabled={saving || disabled}
								onCheckedChange={(checked) => {
									setSelectedNodeIds((current) =>
										checked === true
											? [...current, node.node_id]
											: current.filter((nodeId) => nodeId !== node.node_id),
									);
								}}
							/>
							<span className="min-w-0 break-words">
								{node.node_name}{" "}
								<span className="font-mono text-xs text-muted-foreground">
									{node.node_id}
								</span>
							</span>
						</label>
					);
				})}
			</div>
			{error ? (
				<p className="break-words text-sm text-destructive">{error}</p>
			) : null}
			<div className="flex flex-wrap justify-end gap-2">
				<Button
					variant="secondary"
					disabled={saving}
					onClick={() => {
						setEditing(false);
						setError(null);
					}}
				>
					Cancel
				</Button>
				<Button
					loading={saving}
					disabled={disabled}
					onClick={() => void save()}
				>
					Save membership
				</Button>
			</div>
		</section>
	);
}
