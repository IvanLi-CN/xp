import type { Dispatch, SetStateAction } from "react";
import type { MihomoSmuxConfig } from "../api/adminEndpoints";
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
		<details className="rounded-xl border border-border/70 bg-muted/35">
			<summary className="cursor-pointer list-none px-4 py-3 text-sm font-medium">
				高级设置：连接复用 (SMux)
			</summary>
			<div className="space-y-4 border-t border-border/70 px-4 py-4">
				<p className="text-xs text-muted-foreground">
					仅写入 YAML 订阅，要求 Mihomo &gt;= v1.19.29。VLESS/SS URI
					不包含此设置。
				</p>
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
				</div>
				<div className="grid gap-4 md:grid-cols-2">
					<div className="xp-field-stack">
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
						<p className="text-xs opacity-70">1-16 条连接。</p>
					</div>
					<div className="xp-field-stack">
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
						<p className="text-xs opacity-70">1-64 个并发流。</p>
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
