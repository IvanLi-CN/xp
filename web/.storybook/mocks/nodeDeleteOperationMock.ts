import type { AdminMembershipOperation } from "../../src/api/adminNodes";

export type StorybookNodeDeleteAccepted = {
	nodeId: string;
	operation: AdminMembershipOperation;
};

type JsonResponse = (
	data: unknown,
	init?: { status?: number; headers?: Record<string, string> },
) => Response;

type ErrorResponse = (
	status: number,
	code: string,
	message: string,
) => Response;

const acceptedByState = new WeakMap<object, StorybookNodeDeleteAccepted>();

export function configureNodeDeleteOperationMock(
	state: object,
	accepted: StorybookNodeDeleteAccepted | undefined,
): void {
	if (accepted) acceptedByState.set(state, accepted);
	else acceptedByState.delete(state);
}

export function membershipOperationResponse(
	state: object,
	path: string,
	method: string,
	jsonResponse: JsonResponse,
	errorResponse: ErrorResponse,
): Response | undefined {
	const match = path.match(/^\/api\/admin\/membership-operations\/([^/]+)$/);
	if (!match || method !== "GET") return undefined;
	const operationId = decodeURIComponent(match[1]);
	const operation = acceptedByState.get(state)?.operation;
	if (!operation || operation.operation_id !== operationId) {
		return errorResponse(404, "not_found", "membership operation not found");
	}
	return jsonResponse({ operation: structuredClone(operation) });
}

export function nodeDeleteAcceptedResponse(
	state: object,
	nodeId: string,
	jsonResponse: JsonResponse,
): Response | undefined {
	const accepted = acceptedByState.get(state);
	if (accepted?.nodeId !== nodeId) return undefined;
	return jsonResponse(
		{
			operation_id: accepted.operation.operation_id,
			phase: accepted.operation.phase,
			status_url: `/api/admin/membership-operations/${accepted.operation.operation_id}`,
		},
		{ status: 202 },
	);
}
