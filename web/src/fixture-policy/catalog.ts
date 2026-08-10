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

type SlotIndex<
	Limit extends number,
	Indexes extends number[] = [],
> = Indexes["length"] extends Limit
	? Indexes[number]
	: SlotIndex<Limit, [...Indexes, Indexes["length"]]>;

const slotString = Object.fromEntries(
	catalog.slots.strings.map(stringSlotEntry),
) as Record<`s${SlotIndex<685>}`, () => string>;
const slotNumber = Object.fromEntries(
	catalog.slots.numbers.map(numberSlotEntry),
) as Record<`n${SlotIndex<42>}`, () => number>;
const slotList = Object.fromEntries(
	catalog.slots.stringLists.map(listSlotEntry),
) as Record<`l${SlotIndex<38>}`, () => string[]>;

function createSubscriptionTokenFactory() {
	let subscriptionTokenIndex = 0;
	return () => {
		const token = catalog.subscription.tokens[subscriptionTokenIndex];
		if (!token) {
			throw new Error("synthetic subscription token catalog exhausted");
		}
		subscriptionTokenIndex += 1;
		return token;
	};
}

let nextSubscriptionTokenIndex = 0;

function nextSubscriptionToken() {
	const token = catalog.subscription.tokens[nextSubscriptionTokenIndex];
	if (!token) {
		throw new Error("synthetic subscription token catalog exhausted");
	}
	nextSubscriptionTokenIndex += 1;
	return token;
}

const meshPeerNodeIdIndexes = [
	17, 32, 36, 56, 57, 63, 69, 70, 72, 73, 77, 93, 98, 106, 110, 113, 118, 124,
	134, 145, 149, 153, 182, 187, 188, 189, 190, 206, 213, 220, 224, 229, 233,
	238, 241, 243, 246, 258, 263, 271, 274, 290, 301, 312, 317, 325, 329, 332,
	335, 338,
] as const;
let meshPeerNodeIdIndex = 0;

function nextMeshPeerNodeId() {
	const slotIndex =
		meshPeerNodeIdIndexes[meshPeerNodeIdIndex % meshPeerNodeIdIndexes.length];
	meshPeerNodeIdIndex += 1;
	return catalog.slots.strings[slotIndex];
}

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
		createSubscriptionTokenFactory: () => createSubscriptionTokenFactory(),
		nextSubscriptionToken,
		nextMeshPeerNodeId,
		tokenPrimary: () => catalog.identifiers.tokenPrimary,
		tokenSecondary: () => catalog.identifiers.tokenSecondary,
		tokenTertiary: () => catalog.identifiers.tokenTertiary,
		tokenQuaternary: () => catalog.identifiers.tokenQuaternary,
		tokenQuinary: () => catalog.identifiers.tokenQuinary,
		probeRunPrimary: () => catalog.identifiers.probeRunPrimary,
		probeRunSecondary: () => catalog.identifiers.probeRunSecondary,
		probeConfigPrimary: () => catalog.identifiers.probeConfigPrimary,
		clusterPrimary: () => catalog.identifiers.clusterPrimary,
		endpointTagPrimary: () => catalog.identifiers.endpointTagPrimary,
		endpointTagSecondary: () => catalog.identifiers.endpointTagSecondary,
		endpointTagTertiary: () => catalog.identifiers.endpointTagTertiary,
		endpointTagMissing: () => catalog.identifiers.endpointTagMissing,
	},
	timestamp: {
		earlier: () => catalog.timestamps.earlier,
		baseline: () => catalog.timestamps.baseline,
		recent: () => catalog.timestamps.recent,
		later: () => catalog.timestamps.later,
		probeHour: () => catalog.timestamps.probeHour,
		probeLatest: () => catalog.timestamps.probeLatest,
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
	endpoint: {
		vlessKind: () =>
			catalog.operations.endpoint.vlessKind as "vless_reality_vision_tcp",
		ssKind: () =>
			catalog.operations.endpoint.ssKind as "ss2022_2022_blake3_aes_128_gcm",
		port443: () => catalog.operations.endpoint.port443,
		port8443: () => catalog.operations.endpoint.port8443,
		port9443: () => catalog.operations.endpoint.port9443,
		port53844: () => catalog.operations.endpoint.port53844,
		reality: () => catalog.operations.endpoint.reality,
		realityAlternate: () => catalog.operations.endpoint.realityAlternate,
		realityKeys: () => catalog.operations.endpoint.realityKeys,
		shortIds: () => catalog.operations.endpoint.shortIds,
		activeShortId: () => catalog.operations.endpoint.activeShortId,
		serverPskB64: () => catalog.operations.endpoint.serverPskB64,
		serverPskB64Alternate: () =>
			catalog.operations.endpoint.serverPskB64Alternate,
		serverPskB64Escaped: () => catalog.operations.endpoint.serverPskB64Escaped,
		userPskB64: () => catalog.operations.endpoint.userPskB64,
		authority53844: () => catalog.operations.endpoint.authority53844,
		authorityAlias: () => catalog.operations.endpoint.authorityAlias,
		canaryH2c: () => catalog.operations.endpoint.canaryH2c as "h2c",
	},
	quota: {
		limitBytes: () => catalog.operations.quota.limitBytes,
		usedBytes: () => catalog.operations.quota.usedBytes,
		remainingBytes: () => catalog.operations.quota.remainingBytes,
		fiveGiB: () => catalog.operations.quota.fiveGiB,
		tenGiB: () => catalog.operations.quota.tenGiB,
		elevenGiB: () => catalog.operations.quota.elevenGiB,
		fifteenGiB: () => catalog.operations.quota.fifteenGiB,
		fourGiB: () => catalog.operations.quota.fourGiB,
		oneGiB: () => catalog.operations.quota.oneGiB,
		reset: () => catalog.operations.quota.reset,
		resetSource: () => catalog.operations.quota.resetSource as "user",
	},
	user: {
		credentialEpoch: () => catalog.operations.user.credentialEpoch,
		priorityTierDefault: () =>
			catalog.operations.user.priorityTierDefault as "p3",
		priorityTierCreated: () =>
			catalog.operations.user.priorityTierCreated as "p2",
	},
	subscription: {
		rawUri: () => catalog.operations.subscription.rawUri,
		clash: () => catalog.operations.subscription.clashLines.join("\n"),
		providerHost: () => catalog.operations.subscription.providerHost,
		providerPassword: () => catalog.operations.subscription.providerPassword,
	},
	list: {
		serverName35: () => [catalog.slots.strings[35]],
		primaryServerNames: () => [...catalog.lists.primaryServerNames],
		secondaryServerNames: () => [...catalog.lists.secondaryServerNames],
		tertiaryServerNames: () => [...catalog.lists.tertiaryServerNames],
		primaryAuthorities: () => [...catalog.lists.primaryAuthorities],
		tertiaryAuthorities: () => [...catalog.lists.tertiaryAuthorities],
	},
} as const;
