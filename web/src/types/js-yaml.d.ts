declare module "js-yaml" {
	const yaml: {
		load(input: string): unknown;
		dump(input: unknown): string;
	};

	export default yaml;
}
