import type { ReactNode } from "react";

type PageHeaderProps = {
	title: ReactNode;
	description?: ReactNode;
	actions?: ReactNode;
	meta?: ReactNode;
};

export function PageHeader({
	title,
	description,
	actions,
	meta,
}: PageHeaderProps) {
	return (
		<div className="flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
			<div className="min-w-0">
				<h1 className="text-2xl font-semibold leading-tight">{title}</h1>
				{description ? (
					<p className="mt-1 text-sm opacity-70">{description}</p>
				) : null}
			</div>

			<div className="flex flex-col gap-2 md:items-end">
				{meta ? (
					<div className="flex flex-wrap items-center gap-2">{meta}</div>
				) : null}
				{actions ? (
					<div className="flex flex-wrap items-center gap-2">{actions}</div>
				) : null}
			</div>
		</div>
	);
}
