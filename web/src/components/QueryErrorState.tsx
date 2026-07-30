import type { ReactNode } from "react";

import { PageState } from "./PageState";
import { QueryRetryAction } from "./QueryRetryAction";

type QueryErrorStateProps = {
	title: string;
	description: string;
	error: unknown;
	loading?: boolean;
	disabled?: boolean;
	onRetry: () => void;
	beforeRetry?: ReactNode;
};

export function QueryErrorState({
	title,
	description,
	error,
	loading,
	disabled,
	onRetry,
	beforeRetry,
}: QueryErrorStateProps) {
	return (
		<PageState
			variant="error"
			title={title}
			description={description}
			error={error}
			action={
				beforeRetry ? (
					<>
						{beforeRetry}
						<QueryRetryAction
							loading={loading}
							disabled={disabled}
							onRetry={onRetry}
						/>
					</>
				) : (
					<QueryRetryAction
						loading={loading}
						disabled={disabled}
						onRetry={onRetry}
					/>
				)
			}
		/>
	);
}
