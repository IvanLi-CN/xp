import catalog from "../../../fixture-policy/catalog.json" with {
	type: "json",
};

type StringAccessors<Values extends Record<string, string>> = {
	readonly [Key in keyof Values]: () => Values[Key];
};

type NumberAccessors<Values extends Record<string, number>> = {
	readonly [Key in keyof Values]: () => Values[Key];
};

type ListAccessors<Values extends Record<string, string[]>> = {
	readonly [Key in keyof Values]: () => string[];
};

function createStringAccessors<Values extends Record<string, string>>(
	values: Values,
) {
	return Object.fromEntries(
		Object.entries(values).map(([name, value]) => [name, () => value]),
	) as StringAccessors<Values>;
}

function createNumberAccessors<Values extends Record<string, number>>(
	values: Values,
) {
	return Object.fromEntries(
		Object.entries(values).map(([name, value]) => [name, () => value]),
	) as NumberAccessors<Values>;
}

function createListAccessors<Values extends Record<string, string[]>>(
	values: Values,
) {
	return Object.fromEntries(
		Object.entries(values).map(([name, value]) => [name, () => [...value]]),
	) as ListAccessors<Values>;
}

const fixtureStrings = {
	nodeId: createStringAccessors(catalog.fixtures.strings.nodeId),
	nodeName: createStringAccessors(catalog.fixtures.strings.nodeName),
	endpointId: createStringAccessors(catalog.fixtures.strings.endpointId),
	endpointTag: createStringAccessors(catalog.fixtures.strings.endpointTag),
	token: createStringAccessors(catalog.fixtures.strings.token),
	cluster: createStringAccessors(catalog.fixtures.strings.cluster),
	service: createStringAccessors(catalog.fixtures.strings.service),
	host: createStringAccessors(catalog.fixtures.strings.host),
	timestamp: createStringAccessors(catalog.fixtures.strings.timestamp),
	address: createStringAccessors(catalog.fixtures.strings.address),
	url: createStringAccessors(catalog.fixtures.strings.url),
	identifier: createStringAccessors(catalog.fixtures.strings.identifier),
	label: createStringAccessors(catalog.fixtures.strings.label),
};

const fixtureNumbers = createNumberAccessors(catalog.fixtures.numbers.value);
const fixtureHostLists = createListAccessors(
	catalog.fixtures.stringLists.hostList,
);

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

const meshPeerNodeIds = [
	catalog.fixtures.strings.nodeId.fixture17,
	catalog.fixtures.strings.nodeId.fixture32,
	catalog.fixtures.strings.nodeId.fixture36,
	catalog.fixtures.strings.nodeId.fixture56,
	catalog.fixtures.strings.nodeId.fixture57,
	catalog.fixtures.strings.nodeId.fixture63,
	catalog.fixtures.strings.nodeId.fixture69,
	catalog.fixtures.strings.nodeId.fixture70,
	catalog.fixtures.strings.nodeId.fixture72,
	catalog.fixtures.strings.nodeId.fixture73,
	catalog.fixtures.strings.nodeId.fixture77,
	catalog.fixtures.strings.nodeId.fixture93,
	catalog.fixtures.strings.nodeId.fixture98,
	catalog.fixtures.strings.nodeId.fixture106,
	catalog.fixtures.strings.nodeId.fixture110,
	catalog.fixtures.strings.nodeId.fixture113,
	catalog.fixtures.strings.nodeId.fixture118,
	catalog.fixtures.strings.nodeId.fixture124,
	catalog.fixtures.strings.nodeId.fixture134,
	catalog.fixtures.strings.nodeId.fixture145,
	catalog.fixtures.strings.nodeId.fixture149,
	catalog.fixtures.strings.nodeId.fixture153,
	catalog.fixtures.strings.nodeId.fixture182,
	catalog.fixtures.strings.nodeId.fixture187,
	catalog.fixtures.strings.nodeId.fixture188,
	catalog.fixtures.strings.nodeId.fixture189,
	catalog.fixtures.strings.nodeId.fixture190,
	catalog.fixtures.strings.nodeId.fixture206,
	catalog.fixtures.strings.nodeId.fixture213,
	catalog.fixtures.strings.nodeId.fixture220,
	catalog.fixtures.strings.nodeId.fixture224,
	catalog.fixtures.strings.nodeId.fixture229,
	catalog.fixtures.strings.nodeId.fixture233,
	catalog.fixtures.strings.nodeId.fixture238,
	catalog.fixtures.strings.nodeId.fixture241,
	catalog.fixtures.strings.nodeId.fixture243,
	catalog.fixtures.strings.nodeId.fixture246,
	catalog.fixtures.strings.nodeId.fixture258,
	catalog.fixtures.strings.nodeId.fixture263,
	catalog.fixtures.strings.nodeId.fixture271,
	catalog.fixtures.strings.nodeId.fixture274,
	catalog.fixtures.strings.nodeId.fixture290,
	catalog.fixtures.strings.nodeId.fixture301,
	catalog.fixtures.strings.nodeId.fixture312,
	catalog.fixtures.strings.nodeId.fixture317,
	catalog.fixtures.strings.nodeId.fixture325,
	catalog.fixtures.strings.nodeId.fixture329,
	catalog.fixtures.strings.nodeId.fixture332,
	catalog.fixtures.strings.nodeId.fixture335,
	catalog.fixtures.strings.nodeId.fixture338,
];
let meshPeerNodeIdIndex = 0;

function nextMeshPeerNodeId() {
	const nodeId = meshPeerNodeIds[meshPeerNodeIdIndex % meshPeerNodeIds.length];
	meshPeerNodeIdIndex += 1;
	return nodeId;
}

export const fixtureCatalog = {
	nodeId: fixtureStrings.nodeId,
	nodeName: fixtureStrings.nodeName,
	endpointTag: fixtureStrings.endpointTag,
	token: fixtureStrings.token,
	cluster: fixtureStrings.cluster,
	service: fixtureStrings.service,
	label: fixtureStrings.label,
	number: fixtureNumbers,
	hostList: fixtureHostLists,
	optional: {
		none: () => null,
		undefined: () => undefined,
	},
	string: {
		none: () => catalog.subscription.accessHosts[18],
	},
	host: {
		...fixtureStrings.host,
		primary: () => catalog.hosts.primary,
		secondary: () => catalog.hosts.secondary,
		tertiary: () => catalog.hosts.tertiary,
		serverPrimary: () => catalog.hosts.serverPrimary,
		serverSecondary: () => catalog.hosts.serverSecondary,
	},
	address: {
		...fixtureStrings.address,
		primaryIpv4: () => catalog.addresses.primaryIpv4,
		secondaryIpv4: () => catalog.addresses.secondaryIpv4,
		tertiaryIpv4: () => catalog.addresses.tertiaryIpv4,
		loopback: () => catalog.addresses.loopback,
		loopback39043: () => catalog.addresses.loopback39043,
		loopback49043: () => catalog.addresses.loopback49043,
	},
	url: {
		...fixtureStrings.url,
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
		...fixtureStrings.identifier,
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
		...fixtureStrings.timestamp,
		earlier: () => catalog.timestamps.earlier,
		baseline: () => catalog.timestamps.baseline,
		recent: () => catalog.timestamps.recent,
		later: () => catalog.timestamps.later,
		releasePrevious: () => catalog.timestamps.releasePrevious,
		releaseCurrent: () => catalog.timestamps.releaseCurrent,
		releaseHttp: () => catalog.timestamps.releaseHttp,
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
	endpointId: fixtureStrings.endpointId,
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
		serverName35: () => [catalog.fixtures.strings.host.fixture35],
		primaryServerNames: () => [...catalog.lists.primaryServerNames],
		secondaryServerNames: () => [...catalog.lists.secondaryServerNames],
		tertiaryServerNames: () => [...catalog.lists.tertiaryServerNames],
		primaryAuthorities: () => [...catalog.lists.primaryAuthorities],
		tertiaryAuthorities: () => [...catalog.lists.tertiaryAuthorities],
	},
} as const;
