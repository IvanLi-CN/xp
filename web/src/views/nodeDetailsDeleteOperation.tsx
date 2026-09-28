import { type QueryClient, useQuery } from "@tanstack/react-query";
import { useCallback, useEffect, useRef, useState } from "react";

import {
	type AdminMembershipOperation,
	type AdminNodeDeletePreviewEndpoint,
	deleteAdminNode,
	fetchAdminMembershipOperation,
} from "../api/adminNodes";
import { isBackendApiError } from "../api/backendError";
import { Button } from "../components/Button";
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
			current.state.error || isTerminal(current.state.data?.phase)
				? false
				: 2_500,
		retry: false,
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
		error: query.error,
		isFetching: query.isFetching,
		retry: query.refetch,
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
	const {
		operation,
		operationId,
		setPendingOperation,
		error: operationError,
		isFetching: operationIsFetching,
		retry: retryOperation,
	} = useNodeDeleteOperation({
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
		operationError,
		operationIsFetching,
		retryOperation,
		isDeleting,
		submitDelete,
	};
}

export function NodeDeleteOperationStatus({
	error,
	isFetching,
	operation,
	onRetry,
	visible,
}: {
	error: unknown;
	isFetching: boolean;
	operation: AdminMembershipOperation | undefined;
	onRetry: () => void;
	visible: boolean;
}) {
	if (!visible) return null;
	const hasError = error !== null;
	return (
		<div
			className={alertClass(
				hasError ? "error" : "warning",
				"flex items-center justify-between gap-2 py-2",
			)}
			role={hasError ? "alert" : "status"}
		>
			<div className="min-w-0 space-y-1">
				<div className="flex items-center gap-2">
					<Badge variant={hasError ? "destructive" : "warning"} size="sm">
						{hasError ? "unavailable" : (operation?.phase ?? "pending")}
					</Badge>
					<span>
						{hasError
							? "Node deletion status is unavailable."
							: operation?.phase === "blocked"
								? "Node deletion is blocked."
								: "Node deletion is continuing."}
					</span>
				</div>
				{hasError ? (
					<p className="truncate text-xs opacity-80">
						{formatBackendError(error)}
					</p>
				) : operation?.evidence ? (
					<p className="truncate text-xs opacity-80">{operation.evidence}</p>
				) : null}
			</div>
			{hasError ? (
				<Button
					variant="secondary"
					size="sm"
					loading={isFetching}
					onClick={onRetry}
				>
					Retry status
				</Button>
			) : null}
		</div>
	);
}
