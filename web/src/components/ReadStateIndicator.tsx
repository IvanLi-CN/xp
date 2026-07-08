import { Badge } from "@/components/ui/badge";

import { Icon } from "./Icon";

type ReadStateIndicatorProps = {
	tone: "info" | "warning";
	label: string;
	title?: string;
};

export function ReadStateIndicator({
	tone,
	label,
	title,
}: ReadStateIndicatorProps) {
	return (
		<Badge
			variant={tone === "warning" ? "warning" : "info"}
			size="sm"
			className="gap-1.5 font-medium"
			title={title}
		>
			<Icon
				name={tone === "warning" ? "tabler:wifi-off" : "tabler:database"}
				size={12}
				className="shrink-0"
			/>
			<span>{label}</span>
		</Badge>
	);
}
