# History

- The topic originated in legacy Spec `#s2w9k`.
- It records the completed ratio-editor and membership-mapping work.

## Change log

- Added membership persistence + migration to support node-centric relationship reads.
- Added admin node weight-row aggregate API.
- Added weight write audit log details.
- Reworked quota policy UI to node-centric ratio editing with visual chart and lock-aware controls.
- Added requirement delta: global default allocation + node inherit/override policy switch.
- Adjusted ratio editor responsive threshold to `md`, added viewport/panel tier DOM markers, and tuned compact table spacing for narrow panels.
- Rebalanced table columns (less `User`, more `Slider`) and removed hard table min-width to avoid unnecessary scrollbar in supported viewports.
- Refined table UX by removing the dedicated `Input (%)` column, hiding user id text in table rows, and adding double-click inline percent editing.
