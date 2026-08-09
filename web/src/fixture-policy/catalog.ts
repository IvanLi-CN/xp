import catalog from "../../../fixture-policy/catalog.json" with {
	type: "json",
};

function stringSlotEntry(_value: string, index: number) {
	return [`s${index}`, () => catalog.slots.strings[index]];
}

function numberSlotEntry(_value: number, index: number) {
	return [`n${index}`, () => catalog.slots.numbers[index]];
}

function listSlotEntry(_value: string[], index: number) {
	return [`l${index}`, () => [...catalog.slots.stringLists[index]]];
}

const slotString = Object.fromEntries(
	catalog.slots.strings.map(stringSlotEntry),
) as Record<`s${number}`, () => string>;
const slotNumber = Object.fromEntries(
	catalog.slots.numbers.map(numberSlotEntry),
) as Record<`n${number}`, () => number>;
const slotList = Object.fromEntries(
	catalog.slots.stringLists.map(listSlotEntry),
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
			url: catalog.urls.canaryHttpsListener,
			mode: "auto" as const,
		}),
		httpsAlternate: () => ({
			url: catalog.urls.canaryHttpsAlternate,
			mode: "auto" as const,
		}),
		httpLoopback: () => ({
			url: catalog.urls.canaryHttpLoopback,
			mode: "auto" as const,
		}),
	},
	authority: {
		edgeExamplePort443: () => [...catalog.lists.primaryAuthorities],
		existingAuthoritiesPort443: () => [
			...catalog.lists.existingAuthoritiesPort443,
		],
		existingAndHost119Port53844: () => [
			...catalog.lists.existingAndHost119Port53844,
		],
		host119Port53844: () => [...catalog.lists.host119Port53844],
		host126: () => [...catalog.lists.host126],
		host126Port443: () => [...catalog.lists.host126Port443],
		host126Port53844: () => [...catalog.lists.host126Port53844],
		host130: () => [...catalog.lists.host130],
		host130Port443: () => [...catalog.lists.host130Port443],
		host130Port8443: () => [...catalog.lists.host130Port8443],
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
		date: () => catalog.timestamps.date,
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
