export const NODE_DETAILS_TAB_VALUES = [
	"runtime",
	"resources",
	"metadata",
	"traffic",
	"ipUsage",
	"tcpConnections",
	"settings",
] as const;

export type NodeDetailsTab = (typeof NODE_DETAILS_TAB_VALUES)[number];

export function isNodeDetailsTab(value: string): value is NodeDetailsTab {
	return NODE_DETAILS_TAB_VALUES.includes(value as NodeDetailsTab);
}
