import type { ReactNode } from "react";

export type ResourceTableHeader = {
	key: string;
	label: ReactNode;
};

type ResourceTableProps = {
	headers: ResourceTableHeader[];
	children: ReactNode;
};

export function ResourceTable({ headers, children }: ResourceTableProps) {
	return (
		<div className="overflow-x-auto bg-base-100 shadow rounded-box">
			<table className="table table-zebra">
				<thead>
					<tr>
						{headers.map((header) => (
							<th key={header.key}>{header.label}</th>
						))}
					</tr>
				</thead>
				<tbody>{children}</tbody>
			</table>
		</div>
	);
}
