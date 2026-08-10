import type { UserTrafficNodeOption } from "../api/adminTraffic";

type UserTrafficNodeFilter = {
	activeNodeId: string | null;
	options: UserTrafficNodeOption[];
	optionsUserId: string;
	userId: string;
};

export function resolveUserTrafficNodeFilter({
	activeNodeId,
	options,
	optionsUserId,
	userId,
}: UserTrafficNodeFilter): string | null {
	if (optionsUserId !== userId || activeNodeId === null) return null;
	return options.some((node) => node.node_id === activeNodeId)
		? activeNodeId
		: null;
}
