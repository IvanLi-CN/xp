import tablerIcons from "@iconify-json/tabler/icons.json";
import { Icon as IconifyIcon, addCollection } from "@iconify/react";

let tablerLoaded = false;

function ensureTablerLoaded(): void {
	if (tablerLoaded) return;
	addCollection(tablerIcons);
	tablerLoaded = true;
}

type IconProps = {
	name: string;
	size?: number | string;
	className?: string;
	ariaLabel?: string;
};

export function Icon({ name, size = 18, className, ariaLabel }: IconProps) {
	ensureTablerLoaded();

	if (!name.startsWith("tabler:")) {
		throw new Error(
			`Icon name must use tabler: set for plan #0010, got: ${name}`,
		);
	}

	return (
		<IconifyIcon
			icon={name}
			width={size}
			height={size}
			className={className}
			aria-label={ariaLabel}
		/>
	);
}
