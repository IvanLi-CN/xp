import { fixtureStoryData } from "../fixture-policy/storybook";

export function buildDenseNodeIpUsageStories() {
	return fixtureStoryData.nodeIpUsage();
}

export function buildDenseUserIpUsageStories() {
	return fixtureStoryData.userIpUsage();
}

export function buildDuplicateNameUserIpUsageStories() {
	return fixtureStoryData.duplicateNameUserIpUsage();
}
