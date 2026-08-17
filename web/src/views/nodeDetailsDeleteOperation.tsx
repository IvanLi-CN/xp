import { type QueryClient, useQuery } from "@tanstack/react-query";
import { useCallback, useEffect, useRef, useState } from "react";

import {
	type AdminMembershipOperation,
	type AdminNodeDeletePreviewEndpoint,
	deleteAdminNode,
	fetchAdminMembershipOperation,
} from "../api/adminNodes";
import { isBackendApiError } from "../api/backendError";
import { alertClass } from "../components/ui-helpers";
import { Badge } from "../components/ui/badge";
import { formatBackendError } from "../utils/backendErrorMessage";

const STORAGE_PREFIX = "xp_node_delete_operation_v1";

function storageKey(nodeId: string): string {
	return `${STORAGE_PREFIX}:${nodeId}`;
}

function readPendingOperation(nodeId: string): string | null {
	if (typeof sessionStorage === "undefined") return null;
	return sessionStorage.getItem(storageKey(nodeId));
}

function writePendingOperation(
	nodeId: string,
	operationId: string | null,
): void {
	if (typeof sessionStorage === "undefined") return;
	if (operationId) sessionStorage.setItem(storageKey(nodeId), operationId);
	else sessionStorage.removeItem(storageKey(nodeId));
}

function isTerminal(
	phase: AdminMembershipOperation["phase"] | undefined,
): boolean {
	return phase === "completed" || phase === "blocked" || phase === "expired";
}

type UseNodeDeleteOperationOptions = {
	adminToken: string;
	isOnline: boolean;
	nodeId: string;
	onCompleted: () => void;
};

export function useNodeDeleteOperation({
	adminToken,
	isOnline,
	nodeId,
	onCompleted,
}: UseNodeDeleteOperationOptions) {
	const [operationId, setOperationId] = useState(() =>
		readPendingOperation(nodeId),
	);
	const handledOperationId = useRef<string | null>(null);
	const setPendingOperation = useCallback(
		(nextOperationId: string | null) => {
			writePendingOperation(nodeId, nextOperationId);
			setOperationId(nextOperationId);
		},
		[nodeId],
	);
	useEffect(() => {
		handledOperationId.current = null;
		setOperationId(readPendingOperation(nodeId));
	}, [nodeId]);
	const query = useQuery({
		queryKey: ["adminMembershipOperation", adminToken, operationId],
		enabled: adminToken.length > 0 && operationId !== null && isOnline,
		queryFn: ({ signal }) =>
			fetchAdminMembershipOperation(adminToken, operationId ?? "", signal),
		refetchInterval: (current) =>
			isTerminal(current.state.data?.phase) ? false : 2_500,
	});
	useEffect(() => {
		const operation = query.data;
		if (
			!operation ||
			operation.operation_id === handledOperationId.current ||
			operation.phase !== "completed"
		) {
			return;
		}
		handledOperationId.current = operation.operation_id;
		setPendingOperation(null);
		onCompleted();
	}, [onCompleted, query.data, setPendingOperation]);
	useEffect(() => {
		if (isBackendApiError(query.error) && query.error.status === 404) {
			setPendingOperation(null);
		}
	}, [query.error, setPendingOperation]);

	return {
		operation: query.data,
		operationId,
		setPendingOperation,
	};
}

type UseNodeDeleteFlowOptions = Omit<
	UseNodeDeleteOperationOptions,
	"onCompleted"
> & {
	deletePreviewEndpoints: AdminNodeDeletePreviewEndpoint[];
	navigateToNodes: () => void;
	pushToast: (input: {
		variant: "success" | "info" | "error";
		message: string;
	}) => void;
	queryClient: QueryClient;
	syncCompletedCache: () => void;
};

export function useNodeDeleteFlow({
	adminToken,
	deletePreviewEndpoints,
	isOnline,
	nodeId,
	navigateToNodes,
	pushToast,
	queryClient,
	syncCompletedCache,
}: UseNodeDeleteFlowOptions) {
	const [isDeleting, setIsDeleting] = useState(false);
	const onCompleted = useCallback(() => {
		void queryClient.invalidateQueries({
			queryKey: ["adminNodes", adminToken],
		});
		void queryClient.invalidateQueries({
			queryKey: ["adminEndpoints", adminToken],
		});
		pushToast({ variant: "success", message: "Node deleted." });
		navigateToNodes();
	}, [adminToken, navigateToNodes, pushToast, queryClient]);
	const { operation, operationId, setPendingOperation } =
		useNodeDeleteOperation({
			adminToken,
			isOnline,
			nodeId,
			onCompleted,
		});
	const submitDelete = useCallback(async () => {
		setIsDeleting(true);
		try {
			const result = await deleteAdminNode(adminToken, nodeId, {
				deleteEndpoints: deletePreviewEndpoints.length > 0,
				expectedEndpointIds: deletePreviewEndpoints.map(
					(endpoint) => endpoint.endpoint_id,
				),
			});
			if (result.status === "completed") {
				syncCompletedCache();
				onCompleted();
			} else {
				setPendingOperation(result.operationId);
				pushToast({ variant: "info", message: "Node deletion is continuing." });
			}
		} catch (error) {
			pushToast({ variant: "error", message: formatBackendError(error) });
		} finally {
			setIsDeleting(false);
		}
	}, [
		adminToken,
		deletePreviewEndpoints,
		nodeId,
		onCompleted,
		pushToast,
		setPendingOperation,
		syncCompletedCache,
	]);

	return {
		operation,
		operationId,
		setPendingOperation,
		isDeleting,
		submitDelete,
	};
}

export function NodeDeleteOperationStatus({
	operation,
	visible,
}: {
	operation: AdminMembershipOperation | undefined;
	visible: boolean;
}) {
	if (!visible) return null;
	return (
		<output className={alertClass("warning", "items-center gap-2 py-2")}>
			<Badge variant="warning" size="sm">
				{operation?.phase ?? "pending"}
			</Badge>
			<span>
				{operation?.phase === "blocked"
					? "Node deletion is blocked."
					: "Node deletion is continuing."}
			</span>
			{operation?.evidence ? (
				<span className="text-xs opacity-80">{operation.evidence}</span>
			) : null}
		</output>
	);
}
