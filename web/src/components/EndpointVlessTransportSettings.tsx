import { useId } from "react";

import type { VlessRealityTransport } from "../api/adminEndpoints";
import { cn } from "../lib/utils";
import { InlineHelpTooltip } from "./InlineHelpTooltip";

const TRANSPORT_OPTIONS: ReadonlyArray<{
	value: VlessRealityTransport;
	label: string;
}> = [
	{ value: "xhttp", label: "XHTTP / XMUX" },
	{ value: "vision_tcp", label: "Vision TCP" },
];

const TRANSPORT_FORM_ROW_CLASS =
	"grid gap-2 pt-3 md:grid-cols-[13rem_minmax(0,1fr)] md:items-center md:gap-x-3";
const TRANSPORT_WARNING_BUTTON_CLASS = [
	"inline-flex size-7 shrink-0 items-center justify-center rounded-md",
	"text-warning hover:bg-warning/15 focus-visible:outline-none",
	"focus-visible:ring-[3px] focus-visible:ring-warning/30",
].join(" ");
const XHTTP_HELP_TEXT = [
	"XHTTP/XMUX keeps reusable HTTP/2 transport connections after warm-up.",
	"Mihomo YAML is the recommended subscription format; the raw URI includes its XMUX settings.",
].join(" ");
const TRANSPORT_CHANGE_HELP_TEXT =
	"Changing this mode rebuilds the inbound. Refresh client YAML subscriptions after saving.";

type EndpointVlessTransportSettingsProps = {
	value: VlessRealityTransport;
	onValueChange: (value: VlessRealityTransport) => void;
	visible?: boolean;
	disabled?: boolean;
	existing?: boolean;
	changed?: boolean;
};

export function EndpointVlessTransportSettings({
	value,
	onValueChange,
	visible = true,
	disabled = false,
	existing = false,
	changed = false,
}: EndpointVlessTransportSettingsProps) {
	const id = useId();
	if (!visible) return null;
	const helpText =
		value === "xhttp"
			? XHTTP_HELP_TEXT
			: "Vision TCP uses one proxied TCP stream for each external connection.";
	const changedTransport = existing && changed;
	const tooltipText = changedTransport
		? `${helpText} ${TRANSPORT_CHANGE_HELP_TEXT}`
		: helpText;

	return (
		<details className="border-t border-border/70 pt-3">
			<summary className="cursor-pointer list-none text-sm font-medium">
				Advanced: VLESS transport
			</summary>
			<div className={TRANSPORT_FORM_ROW_CLASS}>
				<div className="flex items-center gap-1">
					<span className="text-sm font-medium">Transport mode</span>
					<InlineHelpTooltip
						className={
							changedTransport ? TRANSPORT_WARNING_BUTTON_CLASS : undefined
						}
						label={
							changedTransport
								? "Transport change impact"
								: "About VLESS transport"
						}
						contentClassName="max-w-56"
						side="top"
					>
						{tooltipText}
					</InlineHelpTooltip>
				</div>
				<fieldset className="min-w-0">
					<div
						aria-label="VLESS transport"
						className={cn(
							"grid min-h-10 w-full grid-cols-2 rounded-lg border",
							"border-border/70 bg-background p-1 shadow-xs sm:max-w-md",
						)}
						role="radiogroup"
					>
						{TRANSPORT_OPTIONS.map((option) => {
							const checked = option.value === value;
							const optionId = `${id}-${option.value}`;
							return (
								<div className="contents" key={option.value}>
									<input
										checked={checked}
										className="peer sr-only"
										disabled={disabled}
										id={optionId}
										name={id}
										onChange={() => onValueChange(option.value)}
										type="radio"
										value={option.value}
									/>
									<label
										className={cn(
											"inline-flex min-w-0 cursor-pointer items-center justify-center",
											"rounded-md px-3 py-1.5 text-center text-sm font-medium",
											"text-muted-foreground transition-colors",
											"peer-focus-visible:ring-[3px] peer-focus-visible:ring-ring/20",
											"peer-disabled:pointer-events-none peer-disabled:opacity-50",
											checked && "bg-primary text-primary-foreground shadow-sm",
											!checked &&
												"hover:bg-accent hover:text-accent-foreground",
										)}
										htmlFor={optionId}
									>
										<span className="min-w-0 break-words">{option.label}</span>
									</label>
								</div>
							);
						})}
					</div>
				</fieldset>
			</div>
		</details>
	);
}
