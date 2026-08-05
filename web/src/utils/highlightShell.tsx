export function highlightShell(text: string) {
	const regex =
		/(\$\{[^}]+\}|\$[A-Za-z_][A-Za-z0-9_]*|'[^']*'|"[^"]*"|https?:\/\/[^\s"']+|--[a-z0-9-]+)/g;
	const parts = text.split(regex);
	let offset = 0;

	return parts.map((part) => {
		if (part.length === 0) return null;
		const key = `o${offset}`;
		offset += part.length;

		let className: string | null = null;
		if (part.startsWith("http://") || part.startsWith("https://")) {
			className = "text-info";
		} else if (part.startsWith("--")) {
			className = "text-warning";
		} else if (part.startsWith("$")) {
			className = "text-accent-foreground";
		} else if (part.startsWith("'") || part.startsWith('"')) {
			className = "text-success";
		}

		return className ? (
			<span key={key} className={className}>
				{part}
			</span>
		) : (
			<span key={key}>{part}</span>
		);
	});
}
