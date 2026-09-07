export function LoginBootstrapCompatibilityPending() {
	return (
		<div className="xp-alert xp-alert-warning px-4 py-3" aria-live="polite">
			<div className="min-w-0 space-y-1">
				<p className="font-medium">Bootstrap compatibility pending.</p>
				<p className="text-muted-foreground">
					The bootstrap node needs a static-console-compatible upgrade or is
					temporarily unavailable. Your token has not been verified or saved.
					Try again after it is ready.
				</p>
			</div>
		</div>
	);
}
