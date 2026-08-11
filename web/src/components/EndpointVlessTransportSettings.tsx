import { useId } from "react";

import type { VlessRealityTransport } from "../api/adminEndpoints";
import { cn } from "../lib/utils";

const TRANSPORT_OPTIONS: ReadonlyArray<{
	value: VlessRealityTransport;
	label: string;
}> = [
	{ value: "xhttp", label: "XHTTP / XMUX" },
	{ value: "vision_tcp", label: "Vision TCP" },
];

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

	return (
		<details className="rounded-xl border border-border/70 bg-muted/35">
			<summary className="cursor-pointer list-none px-4 py-3 text-sm font-medium">
				Advanced: VLESS transport
			</summary>
			<div className="space-y-3 border-t border-border/70 px-4 py-4">
				<fieldset className="min-w-0 space-y-2">
					<legend className="text-sm font-medium">Transport mode</legend>
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

				<p aria-live="polite" className="text-xs text-muted-foreground">
					{value === "xhttp"
						? "Recommended. Mihomo YAML uses one reusable HTTP/2 connection after pool warm-up."
						: "One proxied TCP stream per external connection."}
				</p>
				{value === "xhttp" ? (
					<p className="text-xs text-muted-foreground">
						Raw URI includes Mihomo-specific XMUX settings; YAML remains the
						recommended subscription format.
					</p>
				) : null}
				{existing ? (
					<p
						className={cn(
							"text-xs text-muted-foreground",
							changed && "font-medium text-foreground",
						)}
					>
						Changing this mode rebuilds the inbound. Clients must refresh YAML
						subscriptions after saving.
					</p>
				) : null}
			</div>
		</details>
	);
}
