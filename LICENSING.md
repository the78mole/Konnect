# Licensing

Konnect is free software, licensed under the [GNU AGPL-3.0](LICENSE).

## What that means in practice

**Internal use is free. Always.** Running Konnect inside your company — on any
number of machines, for any commercial purpose, modified or not — triggers no
AGPL obligation, as long as you don't distribute it outside your organization
or let outside users interact with a modified version over a network. Designing
boards with it, manufacturing them, and selling them is use, not distribution:
the boards and design files you produce are yours, license-free.

**If you're an individual** — a hobbyist, a student, a freelancer working on
your own projects, an open-source contributor — everything above applies to you
too. Use Konnect freely.

**The AGPL's obligations begin where distribution begins:**

- If you distribute Konnect (modified or not) outside your organization — say,
  bundled into a product — the AGPL's terms travel with it: recipients get the
  source and the same rights you received.
- If you modify Konnect and let outside users interact with your modified
  version over a network (for example, a hosted design service built on it),
  AGPL §13 requires offering those users your modified source. An unmodified
  Konnect, or a modified one used only internally, carries no such requirement.
- Code that merely *talks to* Konnect — an MCP client, an AI agent, scripts
  driving it — is a separate work; copyleft does not reach across that
  boundary. It becomes a question only when you incorporate Konnect's own code
  into something you distribute or host for others.

## No commercial license from this fork

The original Konnect project offered commercial licenses in addition to the
AGPL. This fork does not: the copyright in the inherited code stays with its
original authors and contributors, and the AGPL-3.0 is the only license under
which this fork receives and passes on that code. If you need terms other than
the AGPL for the original code, you would have to ask its copyright holders.

## Contributions

Contributions to this fork are accepted under the AGPL-3.0 only
("inbound = outbound"); see [CONTRIBUTING.md](CONTRIBUTING.md).
