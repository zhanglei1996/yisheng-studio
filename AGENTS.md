# Repository Instructions

## Code discovery

Prefer codebase-memory-mcp graph tools for code discovery and impact tracing. Re-index after substantial structural changes. Use text search only for literal/config/non-code lookup or when the graph is insufficient.

## Product and visual context

- Product name: 译声工坊.
- Product type: macOS local-first AI video localization desktop app.
- Selected visual truth: `docs/design/references/editor-dark-reference.png`.
- Preserve the professional dark video-editor layout, compact density, four-track timeline, and blue/green/amber/red semantic colors.
- Use Phosphor Icons for UI icons; do not replace visible assets with text glyphs, emoji, CSS drawings, or handcrafted SVG.
- Keep original video data local and make third-party data transmission explicit in product copy.

## Verification

For UI work, run `pnpm typecheck`, `pnpm build`, and the relevant interaction checks. Product Design changes must update `design-qa.md` and pass visual comparison before handoff.
