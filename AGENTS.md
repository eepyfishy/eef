# Project naming

The project names are strictly `eef` and `eefn`, always lowercase, including at
the beginning of sentences. Apply this to documentation, comments, command
messages, UI text, installer branding, and generated display names. Never
automatically capitalize these names or apply uppercase styling to them.

Keep existing machine interfaces compatible: environment-variable identifiers,
wire-format magic, signed/hash-bound verification prefixes, and legacy package
recognition are not display branding. Changing those requires an explicit
compatibility migration, not a text replacement. New display text must use the
lowercase names. Do not rewrite installed files or historical test artifacts.

Preserve user-owned untracked `docs/future/` drafts; apply the naming convention
when incorporating them into maintained documentation.
