import { Icon } from "./Icon";

export function ChartLoadingOverlay() {
	return (
		<output
			className={[
				"pointer-events-none absolute inset-0 z-20 flex items-center",
				"justify-center rounded-[inherit] bg-card/20",
			].join(" ")}
			aria-live="polite"
			aria-label="Loading latest data"
		>
			<div className="flex items-center gap-2 text-sm font-medium text-foreground/80">
				<Icon
					name="tabler:loader-2"
					size={18}
					className="animate-spin motion-reduce:animate-none"
				/>
				<span>Loading latest data...</span>
			</div>
		</output>
	);
}
