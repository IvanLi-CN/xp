import { cn } from "@/lib/utils";
import { Checkbox } from "./ui/checkbox";

type SubscriptionResourceMirrorToggleProps = {
	checked: boolean;
	onCheckedChange: (checked: boolean) => void;
	compact?: boolean;
	disabled?: boolean;
};

export function SubscriptionResourceMirrorToggle({
	checked,
	onCheckedChange,
	compact = false,
	disabled = false,
}: SubscriptionResourceMirrorToggleProps) {
	const id = compact
		? "subscription-resource-mirror-dialog"
		: "subscription-resource-mirror-toolbar";
	return (
		<div
			className={cn(
				"inline-flex min-h-10 items-center gap-2 rounded-xl border border-border",
				"bg-muted/60 px-3 py-2 text-xs font-semibold text-foreground",
			)}
		>
			<Checkbox
				id={id}
				checked={checked}
				disabled={disabled}
				onCheckedChange={(value) => onCheckedChange(value === true)}
			/>
			<label
				className={
					disabled
						? "cursor-not-allowed whitespace-nowrap opacity-60"
						: "cursor-pointer whitespace-nowrap"
				}
				htmlFor={id}
			>
				Use XP mirror for external resources
			</label>
		</div>
	);
}
