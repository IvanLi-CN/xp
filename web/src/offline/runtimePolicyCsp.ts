export function applyRuntimePolicyToCsp(
	csp: string,
	apiOrigins: readonly string[],
): string {
	const origins = [...new Set(apiOrigins)].sort();
	const connectSource = ["connect-src", "'self'", ...origins].join(" ");
	const directives = csp
		.split(";")
		.map((directive) => directive.trim())
		.filter(Boolean);
	const connectIndex = directives.findIndex(
		(directive) =>
			directive.split(/\s+/, 1)[0]?.toLowerCase() === "connect-src",
	);
	if (connectIndex >= 0) directives[connectIndex] = connectSource;
	else directives.push(connectSource);
	return directives.join("; ");
}
