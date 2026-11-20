# Working in this repository

- **The spec.** What the app is meant to do is in `.spec/`, in the format
  described at
  https://raw.githubusercontent.com/madsbuch/better-github/refs/heads/main/spec-llms.txt
  — read that before touching it. Before changing code, find the items whose
  `code` covers it and keep their requirements true. A change in behaviour
  and the change to the spec go together: new requirements for new
  behaviour, a new version (with the reason) for changed behaviour, a
  deprecation (with the reason) for behaviour that goes away. Never delete,
  renumber or reuse a requirement id. `bun run check:spec` checks it.
- **Two apps, one spec.** Polski and Dansk (`madsbuch/learn-danish`) are
  separate codebases that share features, and their specs carry the same
  items and the same requirement ids: a requirement added or changed in one
  is ported to the other, with the feature.
- **The rules.** `PRINCIPLES.md` settles design questions, `DATA.md` how
  data is kept, `DESIGN.md` how screens are built.
