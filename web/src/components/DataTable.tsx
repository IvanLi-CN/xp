import type { ReactNode } from "react";

import {
	Table,
	TableBody,
	TableCell,
	TableHead,
	TableHeader,
	TableRow,
} from "@/components/ui/table";
import { cn } from "@/lib/utils";

import { useUiPrefs } from "./UiPrefs";

export type DataTableHeader = {
	key: string;
	label: ReactNode;
	align?: "left" | "center" | "right";
	className?: string;
};

type DataTableProps = {
	headers: DataTableHeader[];
	children: ReactNode;
	density?: "comfortable" | "compact";
	caption?: ReactNode;
	tableClassName?: string;
};

export function DataTable({
	headers,
	children,
	density,
	caption,
	tableClassName,
}: DataTableProps) {
	const prefs = useUiPrefs();
	const effectiveDensity = density ?? prefs.density;

	return (
		<div className="xp-table-wrap">
			{caption ? (
				<div className="border-b border-border/70 px-4 py-3 text-sm text-muted-foreground">
					{caption}
				</div>
			) : null}
			<Table
				className={cn(
					"xp-table xp-table-zebra",
					effectiveDensity === "compact" && "xp-table-compact",
					tableClassName,
				)}
			>
				<TableHeader>
					<TableRow>
						{headers.map((header) => (
							<TableHead
								key={header.key}
								className={cn(
									header.align === "right"
										? "text-right"
										: header.align === "center"
											? "text-center"
											: "text-left",
									header.className,
								)}
							>
								{header.label}
							</TableHead>
						))}
					</TableRow>
				</TableHeader>
				<TableBody>{children}</TableBody>
			</Table>
		</div>
	);
}

export { TableCell };
