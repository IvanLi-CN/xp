import type { ReactNode } from "react";

type PageStateVariant = "loading" | "empty" | "error";

type PageStateProps = {
	variant: PageStateVariant;
	title: string;
	description?: string;
	action?: ReactNode;
};

const variantTone: Record<PageStateVariant, string> = {
	loading: "text-info",
	empty: "text-base-content",
	error: "text-error",
};

export function PageState({
	variant,
	title,
	description,
	action,
}: PageStateProps) {
	return (
		<div className="card bg-base-100 shadow">
			<div className="card-body items-center text-center">
				<h2 className={`card-title ${variantTone[variant]}`}>{title}</h2>
				{description ? (
					<p className="text-sm opacity-70">{description}</p>
				) : null}
				{variant === "loading" ? (
					<progress className="progress progress-primary w-56" />
				) : null}
				{action ? <div className="card-actions">{action}</div> : null}
			</div>
		</div>
	);
}
