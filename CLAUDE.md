# Working in this repository

- **The spec.** What the app is meant to do is in `.spec/`, in the format
  described at
  https://raw.githubusercontent.com/madsbuch/better-github/refs/heads/main/spec-llms.txt
  — read that before touching it. The spec comes first: a change to what the
  app does starts as a change to the spec, in the same commit as the code —
  a new requirement, a new version of one (with the reason), or a
  deprecation (with the reason). Before changing code, read the
  requirements of the part it belongs to and keep them true. The spec never
  says whether something is built, holds no open questions and points at no
  files. Never delete, renumber or reuse an id. Write little.
  `bun run check:spec` checks it.
- **Two apps, one spec.** Polski and Dansk (`madsbuch/learn-danish`) are
  separate codebases that share features, and their specs carry the same
  items and the same requirement ids: a requirement added or changed in one
  is ported to the other, with the feature.
- **The rules.** `PRINCIPLES.md` settles design questions, `DATA.md` how
  data is kept, `DESIGN.md` how screens are built.
