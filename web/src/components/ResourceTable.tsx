import type { ReactNode } from "react";

import { DataTable } from "./DataTable";
import type { DataTableHeader } from "./DataTable";

export type ResourceTableHeader = DataTableHeader;

type ResourceTableProps = {
	headers: ResourceTableHeader[];
	children: ReactNode;
	tableClassName?: string;
	ariaLabel?: string;
};

export function ResourceTable({
	headers,
	children,
	tableClassName,
	ariaLabel,
}: ResourceTableProps) {
	return (
		<DataTable
			ariaLabel={ariaLabel}
			headers={headers}
			tableClassName={tableClassName}
		>
			{children}
		</DataTable>
	);
}
