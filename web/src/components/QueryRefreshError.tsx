import { AuthRecoveryAction } from "./AuthRecoveryAction";
import { QueryRetryAction } from "./QueryRetryAction";

type QueryRefreshErrorProps = {
	description: string;
	disabled?: boolean;
	error: unknown;
	loading?: boolean;
	onRetry: () => void;
	title: string;
};

export function QueryRefreshError({
	description,
	disabled,
	error,
	loading,
	onRetry,
	title,
}: QueryRefreshErrorProps) {
	return (
		<div
			className="xp-alert xp-alert-error flex items-center justify-between gap-3 px-4 py-2"
			role="alert"
		>
			<div className="min-w-0">
				<p className="font-medium">{title}</p>
				<p className="truncate text-xs opacity-80">{description}</p>
			</div>
			<div className="flex shrink-0 items-center gap-2">
				<AuthRecoveryAction error={error} />
				<QueryRetryAction
					disabled={disabled}
					loading={loading}
					onRetry={onRetry}
				/>
			</div>
		</div>
	);
}
