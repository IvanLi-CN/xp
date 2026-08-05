export function completeSchemas(
	routes: readonly string[],
	known: Readonly<Record<string, readonly string[]>>,
): Readonly<Record<string, readonly string[]>> {
	return Object.fromEntries(routes.map((route) => [route, known[route] ?? []]));
}
