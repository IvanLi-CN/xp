# History

- 2026-06-21: Added a Linux-only node TCP connection history panel that samples system socket-level `ESTABLISHED` inbound TCP connections per business endpoint, stores the last 7 days locally, and exposes a dedicated Node details tab with endpoint multi-select aggregation.
- 2026-11-25: Superseded for full-site classification, total, and layered history by
  `../tcp-connection-history/SPEC.md`; inbound sampling semantics remain referenced.
