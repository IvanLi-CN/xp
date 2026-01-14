import type { ReactNode } from "react";

import { DataTable } from "./DataTable";

export type ResourceTableHeader = {
	key: string;
	label: ReactNode;
};

type ResourceTableProps = {
	headers: ResourceTableHeader[];
	children: ReactNode;
};

export function ResourceTable({ headers, children }: ResourceTableProps) {
	return <DataTable headers={headers}>{children}</DataTable>;
}
