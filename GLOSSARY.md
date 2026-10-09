# XP Mihomo Subscription

This glossary defines XP Mihomo subscription and dynamic proxy-group terms.

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
