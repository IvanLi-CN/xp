import iconAssets from "../../assets-src/icon-assets.json";

export type XpBrandLogoKind = "mark" | "lockup";

type XpBrandLogoProps = {
	kind?: XpBrandLogoKind;
	className?: string;
	alt?: string;
};

const markSrc = `${import.meta.env.BASE_URL}${iconAssets.bicolorSvg}`;
const lockupSrc = `${import.meta.env.BASE_URL}${iconAssets.lockupSvg}`;
const lockupInverseSrc = `${import.meta.env.BASE_URL}${iconAssets.lockupInverseSvg}`;

/** Uses the generated, content-versioned brand assets across app surfaces. */
export function XpBrandLogo({
	kind = "mark",
	className,
	alt = "xp",
}: XpBrandLogoProps) {
	if (kind === "mark") {
		return (
			<img src={markSrc} alt={alt} aria-hidden={!alt} className={className} />
		);
	}

	return (
		<span className={className}>
			<img
				src={lockupSrc}
				alt={alt}
				className="block h-full w-full dark:hidden"
			/>
			<img
				src={lockupInverseSrc}
				alt={alt}
				className="hidden h-full w-full dark:block"
			/>
		</span>
	);
}
