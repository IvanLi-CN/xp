import type { ReactNode } from "react";

import { DataTable } from "./DataTable";

export type ResourceTableHeader = {
	key: string;
	label: ReactNode;
};

type ResourceTableProps = {
	headers: ResourceTableHeader[];
	children: ReactNode;
	tableClassName?: string;
};

export function ResourceTable({
	headers,
	children,
	tableClassName,
}: ResourceTableProps) {
	return (
		<DataTable headers={headers} tableClassName={tableClassName}>
			{children}
		</DataTable>
	);
}
