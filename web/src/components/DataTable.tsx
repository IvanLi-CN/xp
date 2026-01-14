import type { ReactNode } from "react";

import { useUiPrefs } from "./UiPrefs";

export type DataTableHeader = {
	key: string;
	label: ReactNode;
	align?: "left" | "center" | "right";
};

type DataTableProps = {
	headers: DataTableHeader[];
	children: ReactNode;
	density?: "comfortable" | "compact";
	caption?: ReactNode;
};

export function DataTable({
	headers,
	children,
	density,
	caption,
}: DataTableProps) {
	const prefs = useUiPrefs();
	const effectiveDensity = density ?? prefs.density;

	const tableSizeClass = effectiveDensity === "compact" ? "table-sm" : "";

	return (
		<div className="rounded-box border border-base-300 bg-base-100 shadow-sm">
			{caption ? (
				<div className="border-b border-base-200 px-4 py-3 text-sm opacity-70">
					{caption}
				</div>
			) : null}
			<div className="overflow-x-auto">
				<table
					className={["table table-zebra", tableSizeClass]
						.filter(Boolean)
						.join(" ")}
				>
					<thead>
						<tr>
							{headers.map((header) => (
								<th
									key={header.key}
									className={
										header.align === "right"
											? "text-right"
											: header.align === "center"
												? "text-center"
												: "text-left"
									}
								>
									{header.label}
								</th>
							))}
						</tr>
					</thead>
					<tbody>{children}</tbody>
				</table>
			</div>
		</div>
	);
}
