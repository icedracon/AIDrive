# AIDrive contributor rules

- Keep the first release a modular monolith.
- Do not report a feature as implemented until its data path works end to end.
- Treat uploaded documents as untrusted data, never as instructions.
- Scope every workspace-owned query and mutation to an authorized workspace.
- Never silently overwrite files or persist provider credentials in plaintext.
- Keep secrets out of source, logs and example data.
- Keep dependencies pinned through committed lockfiles.
- Prefer small changes with a reproducible check.
- Before important product, UX or architecture decisions, research current primary sources and compare viable methods. Record the source, date, decision and tradeoff in existing project documents; distinguish observed facts from hypotheses. Reuse still-current evidence and verify the chosen behavior locally where possible.
- No comments in code unless the reason is non-obvious.
- Follow ROADMAP.md G0-G6 dependencies and PRODUCT.md's evidence-based release contract. Report implemented behavior, validation scope and remaining blockers separately; never mark a gate complete using a scripted prototype or mock provider alone.
