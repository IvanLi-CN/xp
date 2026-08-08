import catalog from "../../../fixture-policy/catalog.json" with {
	type: "json",
};

const slotString = Object.fromEntries(
	catalog.slots.strings.map((_, index) => [
		`s${index}`,
		() => catalog.slots.strings[index],
	]),
) as Record<`s${number}`, () => string>;
const slotNumber = Object.fromEntries(
	catalog.slots.numbers.map((_, index) => [
		`n${index}`,
		() => catalog.slots.numbers[index],
	]),
) as Record<`n${number}`, () => number>;
const slotList = Object.fromEntries(
	catalog.slots.stringLists.map((_, index) => [
		`l${index}`,
		() => [...catalog.slots.stringLists[index]],
	]),
) as Record<`l${number}`, () => string[]>;

export const fixtureCatalog = {
	slotString,
	slotNumber,
	slotList,
	optional: {
		none: () => null,
		undefined: () => undefined,
	},
	string: {
		none: () => catalog.subscription.accessHosts[18],
	},
	host: {
		primary: () => catalog.hosts.primary,
		secondary: () => catalog.hosts.secondary,
		tertiary: () => catalog.hosts.tertiary,
		serverPrimary: () => catalog.hosts.serverPrimary,
		serverSecondary: () => catalog.hosts.serverSecondary,
	},
	address: {
		primaryIpv4: () => catalog.addresses.primaryIpv4,
		secondaryIpv4: () => catalog.addresses.secondaryIpv4,
		tertiaryIpv4: () => catalog.addresses.tertiaryIpv4,
		loopback: () => catalog.addresses.loopback,
		loopback39043: () => catalog.addresses.loopback39043,
		loopback49043: () => catalog.addresses.loopback49043,
	},
	url: {
		primaryApi: () => catalog.urls.primaryApi,
		secondaryApi: () => catalog.urls.secondaryApi,
		tertiaryApi: () => catalog.urls.tertiaryApi,
		loopback39043: () => catalog.urls.loopback39043,
		publicOrigin: () => catalog.urls.publicOrigin,
		none: () => null,
	},
	canaryUpstream: {
		httpsListener: () => ({
			url: `https://${catalog.addresses.loopback39043}`,
			mode: "auto" as const,
		}),
		httpsAlternate: () => ({
			url: `https://${catalog.addresses.loopback49043}`,
			mode: "auto" as const,
		}),
		httpLoopback: () => ({
			url: `http://${catalog.addresses.loopback}:8080`,
			mode: "auto" as const,
		}),
	},
	authority: {
		edgeExamplePort443: () => [...catalog.lists.primaryAuthorities],
		existingAuthoritiesPort443: () =>
			catalog.slots.stringLists[12].map(
				(_, index) => `${catalog.slots.stringLists[12][index]}:443`,
			),
		existingAndHost119Port53844: () => [
			...catalog.slots.stringLists[12].map(
				(_, index) => `${catalog.slots.stringLists[12][index]}:443`,
			),
			`${catalog.slots.strings[119]}:53844`,
		],
		host119Port53844: () => [`${catalog.slots.strings[119]}:53844`],
		host126: () => [catalog.slots.strings[126]],
		host126Port443: () => [`${catalog.slots.strings[126]}:443`],
		host126Port53844: () => [`${catalog.slots.strings[126]}:53844`],
		host130: () => [catalog.slots.strings[130]],
		host130Port443: () => [`${catalog.slots.strings[130]}:443`],
		host130Port8443: () => [`${catalog.slots.strings[130]}:8443`],
	},
	identifier: {
		nodePrimary: () => catalog.identifiers.nodePrimary,
		nodeSecondary: () => catalog.identifiers.nodeSecondary,
		nodeTertiary: () => catalog.identifiers.nodeTertiary,
		nodeNamePrimary: () => catalog.identifiers.nodeNamePrimary,
		nodeNameSecondary: () => catalog.identifiers.nodeNameSecondary,
		nodeNameTertiary: () => catalog.identifiers.nodeNameTertiary,
		endpointPrimary: () => catalog.identifiers.endpointPrimary,
		endpointSecondary: () => catalog.identifiers.endpointSecondary,
		endpointTertiary: () => catalog.identifiers.endpointTertiary,
		userPrimary: () => catalog.identifiers.userPrimary,
		userSecondary: () => catalog.identifiers.userSecondary,
		userTertiary: () => catalog.identifiers.userTertiary,
		userQuaternary: () => catalog.identifiers.userQuaternary,
		userQuinary: () => catalog.identifiers.userQuinary,
		tokenPrimary: () => catalog.identifiers.tokenPrimary,
		tokenSecondary: () => catalog.identifiers.tokenSecondary,
		tokenTertiary: () => catalog.identifiers.tokenTertiary,
		tokenQuaternary: () => catalog.identifiers.tokenQuaternary,
		tokenQuinary: () => catalog.identifiers.tokenQuinary,
		clusterPrimary: () => catalog.identifiers.clusterPrimary,
		endpointTagPrimary: () => catalog.identifiers.endpointTagPrimary,
		endpointTagSecondary: () => catalog.identifiers.endpointTagSecondary,
		endpointTagTertiary: () => catalog.identifiers.endpointTagTertiary,
	},
	timestamp: {
		earlier: () => catalog.timestamps.earlier,
		baseline: () => catalog.timestamps.baseline,
		recent: () => catalog.timestamps.recent,
		later: () => catalog.timestamps.later,
		none: () => null,
	},
	metric: {
		latencyLow: () => catalog.metrics.latencyLow,
		latencyHigh: () => catalog.metrics.latencyHigh,
		trafficBytes: () => catalog.metrics.trafficBytes,
		availabilityLow: () => catalog.metrics.availabilityLow,
		availabilityHigh: () => catalog.metrics.availabilityHigh,
		availabilityFull: () => catalog.metrics.availabilityFull,
		none: () => null,
	},
	list: {
		primaryServerNames: () => [...catalog.lists.primaryServerNames],
		secondaryServerNames: () => [...catalog.lists.secondaryServerNames],
		tertiaryServerNames: () => [...catalog.lists.tertiaryServerNames],
		primaryAuthorities: () => [...catalog.lists.primaryAuthorities],
		tertiaryAuthorities: () => [...catalog.lists.tertiaryAuthorities],
	},
} as const;
