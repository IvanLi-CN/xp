import { Icon } from "./Icon";

export function ChartLoadingOverlay() {
	return (
		<output
			className={[
				"pointer-events-none absolute inset-0 z-20 flex items-center",
				"justify-center rounded-[inherit] bg-background/45 backdrop-blur-[1px]",
			].join(" ")}
			aria-live="polite"
			aria-label="Loading latest data"
		>
			<div
				className={[
					"flex items-center gap-2 rounded-full border border-border/80",
					"bg-card/95 px-3 py-2 text-sm font-semibold text-foreground shadow-sm",
				].join(" ")}
			>
				<Icon
					name="tabler:loader-2"
					size={20}
					className="animate-spin motion-reduce:animate-none"
				/>
				<span>Loading latest data...</span>
			</div>
		</output>
	);
}
