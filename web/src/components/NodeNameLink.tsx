import { Link } from "@tanstack/react-router";

function resolvedNodeName(nodeName?: string | null): string | null {
	const trimmed = nodeName?.trim();
	return trimmed ? trimmed : null;
}

export function nodeDisplayName(
	nodeId: string,
	nodeName?: string | null,
): string {
	return resolvedNodeName(nodeName) ?? nodeId;
}

export function compareNodeIdsByDisplayName(
	firstNodeId: string,
	secondNodeId: string,
	nodeNamesById: ReadonlyMap<string, string> | undefined,
): number {
	const firstName = nodeDisplayName(
		firstNodeId,
		nodeNamesById?.get(firstNodeId),
	);
	const secondName = nodeDisplayName(
		secondNodeId,
		nodeNamesById?.get(secondNodeId),
	);

	return (
		firstName.localeCompare(secondName) ||
		firstNodeId.localeCompare(secondNodeId)
	);
}

export function NodeNameLink(props: {
	nodeId: string;
	nodeName?: string | null;
}) {
	const nodeName = resolvedNodeName(props.nodeName);
	if (!nodeName) {
		return <span className="font-mono text-xs">{props.nodeId}</span>;
	}

	return (
		<Link
			className="xp-link text-sm"
			to="/nodes/$nodeId"
			params={{ nodeId: props.nodeId }}
			title={props.nodeId}
			aria-label={`Open node details: ${nodeName} (${props.nodeId})`}
		>
			{nodeName}
		</Link>
	);
}
