import type { Dispatch, SetStateAction } from "react";
import type { MihomoSmuxConfig } from "../api/adminEndpoints";
import { InlineHelpTooltip } from "./InlineHelpTooltip";
import { Checkbox } from "./ui/checkbox";
import { Input } from "./ui/input";

type EndpointMihomoSmuxSettingsProps = {
	config: MihomoSmuxConfig;
	available: boolean;
	disabled: boolean;
	inputClass: string;
	maxConnections: string;
	minStreams: string;
	onConfigChange: Dispatch<SetStateAction<MihomoSmuxConfig>>;
	onMaxConnectionsChange: (value: string) => void;
	onMinStreamsChange: (value: string) => void;
};

export function EndpointMihomoSmuxSettings({
	config,
	available,
	disabled,
	inputClass,
	maxConnections,
	minStreams,
	onConfigChange,
	onMaxConnectionsChange,
	onMinStreamsChange,
}: EndpointMihomoSmuxSettingsProps) {
	if (!available) {
		return (
			<p className="text-xs text-muted-foreground">
				This server does not support per-endpoint Mihomo SMux settings.
			</p>
		);
	}

	const controlsDisabled = disabled || !config.enabled;

	return (
		<details className="border-t border-border/70 pt-3">
			<summary className="cursor-pointer list-none text-sm font-medium">
				高级设置：SS2022 连接复用 (SMux)
			</summary>
			<div className="space-y-3 pt-3">
				<div className="flex items-center gap-2">
					<Checkbox
						id="mihomo-smux-enabled"
						checked={config.enabled}
						disabled={disabled}
						onCheckedChange={(checked) =>
							onConfigChange((current) => ({
								...current,
								enabled: checked === true,
							}))
						}
					/>
					<label
						className="cursor-pointer text-sm font-medium"
						htmlFor="mihomo-smux-enabled"
					>
						启用 SMux
					</label>
					<InlineHelpTooltip label="About SS2022 SMux">
						Only emitted in SS2022 Mihomo YAML and requires Mihomo v1.19.29 or
						later. URI output does not include this setting.
					</InlineHelpTooltip>
				</div>
				<div className="grid gap-3 md:grid-cols-2">
					<div className="grid grid-cols-[minmax(0,1fr)_5rem] items-center gap-3">
						<label
							className="text-sm font-medium"
							htmlFor="mihomo-smux-max-connections"
						>
							最大物理连接数
						</label>
						<Input
							id="mihomo-smux-max-connections"
							type="number"
							className={inputClass}
							value={maxConnections}
							min={1}
							max={16}
							disabled={controlsDisabled}
							onChange={(event) => onMaxConnectionsChange(event.target.value)}
						/>
					</div>
					<div className="grid grid-cols-[minmax(0,1fr)_5rem] items-center gap-3">
						<label
							className="text-sm font-medium"
							htmlFor="mihomo-smux-min-streams"
						>
							扩容前最小流数
						</label>
						<Input
							id="mihomo-smux-min-streams"
							type="number"
							className={inputClass}
							value={minStreams}
							min={1}
							max={64}
							disabled={controlsDisabled}
							onChange={(event) => onMinStreamsChange(event.target.value)}
						/>
					</div>
				</div>
				<div className="flex items-center gap-2">
					<Checkbox
						id="mihomo-smux-only-tcp"
						checked={config.only_tcp}
						disabled={controlsDisabled}
						onCheckedChange={(checked) =>
							onConfigChange((current) => ({
								...current,
								only_tcp: checked === true,
							}))
						}
					/>
					<label
						className="cursor-pointer text-sm font-medium"
						htmlFor="mihomo-smux-only-tcp"
					>
						仅复用 TCP
					</label>
				</div>
			</div>
		</details>
	);
}
