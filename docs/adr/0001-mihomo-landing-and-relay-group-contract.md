# Mihomo Landing And Relay Group Contract

The Mihomo subscription puts every generated `🛬 {base}` before regional candidates in `🤯 All`.
Each system `🛣️ {relay-base}` uses other subscribed nodes' access points and excludes its target.
An unsubscribed cluster node is absent from that user's output and cannot be a relay candidate.
With no Access Point, the relay stays fail-closed with `REJECT`, never the target or `DIRECT`.

## Consequences

- `🤯 All` remains hidden `url-test`, with Landing Groups first and regional groups after them.
- Relay candidates use memberships; user-defined `🛣️` groups stay outside this contract.
- External providers may add candidates, but cannot replace the Access Point or bypass `REJECT`.
