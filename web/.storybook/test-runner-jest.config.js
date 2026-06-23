const path = require("node:path");
const { getJestConfig } = require("@storybook/test-runner");

const testRunnerConfig = getJestConfig();
const projectRoot = path.resolve(__dirname, "..");

module.exports = {
	...testRunnerConfig,
	rootDir: projectRoot,
	roots: [projectRoot],
};
