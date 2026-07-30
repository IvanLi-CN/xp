import { Button } from "./Button";

type QueryRetryActionProps = {
	loading?: boolean;
	disabled?: boolean;
	onRetry: () => void;
};

export function QueryRetryAction({
	loading = false,
	disabled = false,
	onRetry,
}: QueryRetryActionProps) {
	return (
		<Button
			variant="secondary"
			loading={loading}
			disabled={disabled}
			onClick={onRetry}
		>
			Retry
		</Button>
	);
}
