# Notice

KopperDigger (`kidig-mcp`) — AI-assisted PCB design for KiCAD 10.
A fork of Konnect.

Copyright (C) 2026 the original Konnect authors and contributors
(see the git history of https://github.com/mixelpixx/Konnect).
Copyright (C) 2026 the contributors to this fork.

This program is free software: you can redistribute it and/or modify it under
the terms of the GNU Affero General Public License as published by the Free
Software Foundation, version 3 only (AGPL-3.0-only). It is distributed WITHOUT
ANY WARRANTY; see [LICENSE](LICENSE) for details.

## This is a modified version

As required by AGPL-3.0 §5(a), this notice states that this repository is a
modified version of the original Konnect, renamed to KopperDigger
(technical name `kidig-mcp`) in November 2026.

- **Origin:** https://github.com/mixelpixx/Konnect, last AGPL-3.0 state
  (commit `676efce`, 1 November 2026). The original repository stopped
  development when its authors moved to a closed-source successor
  (Konnect-EDA by Orchis.ai LLC). Nothing from that successor is included here.
- **Fork:** https://github.com/the78mole/Konnect, maintained independently
  since November 2026. The full list of changes is the git history after
  commit `676efce`.

This fork is not affiliated with, endorsed by, or supported by the original
authors or Orchis.ai LLC. The names of the original authors and projects are
used only to credit them and to describe where the code comes from.

### Rename

The project, binary, crates, configuration paths (`kidig-mcp.toml`,
`~/.kidig-mcp`), environment variables (`KIDIG_MCP_*`) and the KiCad PCM
identifier (`com.github.the78mole.kidig-mcp`) were renamed from Konnect so
that this fork cannot be mistaken for the original project or its commercial
successor. Configuration written for Konnect (`konnect.toml`, `KONNECT_*`) is
not read by KopperDigger.

Third-party code and data included in KopperDigger are listed in
[THIRD_PARTY.md](THIRD_PARTY.md).
