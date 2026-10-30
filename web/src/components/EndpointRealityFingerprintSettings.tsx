import { AdvancedSettings } from "./AdvancedSettings";
import { InlineHelpTooltip } from "./InlineHelpTooltip";
import { Input } from "./ui/input";

type Props = {
	value: string;
	disabled: boolean;
	inputClass: string;
	onChange: (value: string) => void;
};

export function EndpointRealityFingerprintSettings({
	value,
	disabled,
	inputClass,
	onChange,
}: Props) {
	return (
		<AdvancedSettings title="Advanced (optional)">
			<div className="grid gap-2 pt-3 md:grid-cols-[13rem_minmax(0,1fr)] md:items-center md:gap-x-3">
				<div className="flex items-center gap-1">
					<span className="text-sm font-medium font-mono">fingerprint</span>
					<InlineHelpTooltip label="About REALITY fingerprint">
						Defaults to chrome.
					</InlineHelpTooltip>
				</div>
				<Input
					type="text"
					aria-label="fingerprint"
					className={inputClass}
					value={value}
					disabled={disabled}
					placeholder="chrome"
					onChange={(event) => onChange(event.target.value)}
				/>
			</div>
		</AdvancedSettings>
	);
}
