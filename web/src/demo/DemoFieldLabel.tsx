type DemoFieldLabelProps = {
	className?: string;
	htmlFor: string;
	children: string;
};

export function DemoFieldLabel({
	className = "text-sm font-medium",
	htmlFor,
	children,
}: DemoFieldLabelProps) {
	return (
		<label className={className} htmlFor={htmlFor}>
			{children}
		</label>
	);
}
