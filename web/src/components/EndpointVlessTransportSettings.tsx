import { useId } from "react";
import type { MihomoXmuxDraft } from "../utils/mihomoXmux";
import { Input } from "./ui/input";
import { Slider } from "./ui/slider";

import type { VlessRealityTransport } from "../api/adminEndpoints";
import { cn } from "../lib/utils";
import { AdvancedSettings } from "./AdvancedSettings";
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
	xmux?: MihomoXmuxDraft;
	onXmuxChange?: (value: MihomoXmuxDraft) => void;
};

export function EndpointVlessTransportSettings({
	value,
	onValueChange,
	visible = true,
	disabled = false,
	existing = false,
	changed = false,
	xmux,
	onXmuxChange,
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
		<AdvancedSettings title="Advanced: VLESS transport">
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
			{value === "xhttp" && xmux && onXmuxChange ? (
				<div className="space-y-3 border-t border-border/70 mt-4 pt-4">
					<fieldset disabled={disabled} className="flex flex-col gap-2">
						<legend className="text-sm font-medium">XMUX strategy</legend>
						<div
							className={cn(
								"grid w-full grid-cols-2 rounded-lg border",
								"border-border/70 bg-background p-1 shadow-xs",
							)}
						>
							{(
								[
									["connections", "Limit total connections"],
									["concurrency", "Per-connection concurrency"],
								] as const
							).map(([strategy, label]) => (
								<div className="contents" key={strategy}>
									<input
										type="radio"
										id={`${id}-strategy-${strategy}`}
										name={`${id}-strategy`}
										className="peer sr-only"
										checked={xmux.strategy === strategy}
										onChange={() => onXmuxChange({ ...xmux, strategy })}
									/>
									<label
										htmlFor={`${id}-strategy-${strategy}`}
										className={cn(
											"inline-flex min-w-0 cursor-pointer items-center justify-center rounded-md px-3 py-1.5",
											"text-center text-sm font-medium transition-colors",
											"peer-focus-visible:ring-[3px] peer-focus-visible:ring-ring/20",
											"peer-disabled:pointer-events-none peer-disabled:opacity-50",
											xmux.strategy === strategy
												? "bg-primary text-primary-foreground shadow-sm"
												: "text-muted-foreground hover:bg-accent hover:text-accent-foreground",
										)}
									>
										{label}
									</label>
								</div>
							))}
						</div>
					</fieldset>
					{(
						[
							[
								xmux.strategy === "connections"
									? "maxConnections"
									: "maxConcurrency",
								xmux.strategy === "connections"
									? "Maximum connections"
									: "Maximum concurrency",
								xmux.strategy === "connections"
									? xmux.maxConnections
									: xmux.maxConcurrency,
								1,
								xmux.strategy === "connections" ? 32 : 128,
							],
							["reuseTimes", "Maximum reuse times", xmux.reuseTimes, 0, 1024],
							[
								"reusableSecs",
								"Maximum reusable time (seconds)",
								xmux.reusableSecs,
								0,
								7200,
							],
							[
								"keepAlive",
								"Idle keepalive period (seconds)",
								xmux.keepAlive,
								-1,
								300,
							],
						] as const
					).map(([key, label, fieldValue, minimum, defaultMax]) => {
						const parts = fieldValue.split("-");
						const isNegativeOne = key === "keepAlive" && fieldValue === "-1";
						const valid =
							isNegativeOne ||
							(/^(?:0|[1-9]\d*)(?:-(?:0|[1-9]\d*))?$/.test(fieldValue) &&
								(key !== "keepAlive" || parts.length === 1));
						const numbers = isNegativeOne ? [-1] : parts.map(Number);
						const sliderMin =
							numbers.length > 1 ? Math.max(1, minimum) : minimum;
						const validNumbers =
							valid &&
							numbers.every(
								(number) =>
									Number.isSafeInteger(number) &&
									number >= sliderMin &&
									number <= 2147483647,
							) &&
							(numbers.length === 1 || numbers[0] <= numbers[1]);
						const sliderValues = validNumbers ? numbers : [sliderMin];
						const sliderMax = Math.max(defaultMax, ...sliderValues);
						return (
							<div className={TRANSPORT_FORM_ROW_CLASS} key={key}>
								<label className="text-sm font-medium" htmlFor={`${id}-${key}`}>
									{label}
								</label>
								<div className="flex min-w-0 items-center gap-4">
									<Slider
										label={`${label} adjustment`}
										value={sliderValues}
										min={sliderMin}
										max={sliderMax}
										step={1}
										disabled={disabled || !validNumbers}
										onValueChange={(values) =>
											onXmuxChange({ ...xmux, [key]: values.join("-") })
										}
									/>
									<Input
										id={`${id}-${key}`}
										className="w-28 shrink-0"
										value={fieldValue}
										disabled={disabled}
										onChange={(event) =>
											onXmuxChange({ ...xmux, [key]: event.target.value })
										}
									/>
								</div>
							</div>
						);
					})}
					<p className="text-xs text-muted-foreground">
						Enter a number or an ascending range (for example 16-32); range
						values use two slider handles. Zero disables reuse or time limits.
						Keepalive: -1 disables, 0 uses the client default, a positive value
						sets seconds.
					</p>
					<p className="text-xs text-muted-foreground">
						Refresh client subscriptions after saving XMUX changes.
					</p>
				</div>
			) : null}
		</AdvancedSettings>
	);
}
