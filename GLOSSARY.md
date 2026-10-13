# XP Mihomo Subscription

This glossary defines XP Mihomo subscription, dynamic proxy-group, and external resource terms.

## Language

**Subscription Node**:
A node with an endpoint membership in the current user's subscription.
_Avoid_: Unsubscribed node, invisible node

**Access Point**:
The direct Reality proxy generated for a Subscription Node and exposed as its `*-reality` entry.
_Avoid_: Relay node, landing group

**Landing Group**:
The `🛬 {base}` group for one node base and its direct and chain candidates.
_Avoid_: Access point group, relay group

**Relay Group**:
The system `🛣️ {relay-base}` group used by chains; candidates are other nodes' Access Points.
_Avoid_: Landing group, external relay

**Target Node**:
The node a chain proxy ultimately reaches; its own Access Point is excluded from that Relay Group.
_Avoid_: Current node, serving node

### External Resources

**External Resource**:
A Geo data file or provider document obtained outside XP's generated system provider.
_Avoid_: Subscription node, upstream proxy

**Resource Mirror**:
An XP-served address for an External Resource that lets Mihomo obtain the resource through XP.
_Avoid_: Generic reverse proxy, cached copy

**Mirror Mode**:
The explicit delivery mode that replaces eligible External Resource URLs with Resource Mirror URLs.
_Avoid_: Proxy mode, relay mode

**Original Resource URL**:
The source address for an External Resource before Mirror Mode is applied.
_Avoid_: Mirror URL, source node

**GeoX Resource**:
A Mihomo Geo data resource named in `geox-url`, such as `geoip`, `geosite`, `mmdb`, or `asn`.
_Avoid_: Geo proxy, Geo node

**Resource Directory**:
The set of External Resources currently available through XP's Resource Mirror addresses.
_Avoid_: Resource cache, URL registry

**Canonical Mihomo Subscription**:
The user-facing Mihomo configuration that references XP's system provider.
_Avoid_: System provider payload, raw subscription

**System Provider Payload**:
XP's generated proxy document loaded by the Canonical Mihomo Subscription.
_Avoid_: Canonical subscription, external resource

**Mirror Origin**:
The public HTTP origin used in a subscription response to construct Resource Mirror addresses.
_Avoid_: Upstream origin, cluster primary

**Mirrorable Resource**:
An External Resource eligible for access through XP's Resource Mirror.
_Avoid_: Any URL, proxied resource

## Web Demo

**Demo Runtime**:
An explicit Web build/runtime that uses the formal XP page tree with an in-memory mock backend and never falls back to a live backend.
_Avoid_: Demo mode query switch, fake page tree

**Demo Scene**:
A deterministic dataset and fault profile selected by the Demo URL and rebuilt in memory.
_Avoid_: Demo fixture page, saved demo workspace

**Demo Persona**:
A simulated authentication state used to exercise the formal permission surface, such as administrator or unauthenticated.
_Avoid_: Demo role, demo account

**Demo Inspector**:
The local-only control surface for Demo scene, persona, network, data, actions and share state.
_Avoid_: Product settings, query-string toolbar

**Demo Share State**:
The allowlisted URL parameters that reproduce a Demo scene, persona, fault profile and product page without carrying secrets or mutation payloads.
_Avoid_: Serialized demo state, shareable credentials
