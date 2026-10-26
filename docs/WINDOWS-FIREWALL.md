# Windows firewall and development test launches

Keep Windows Firewall and its notifications enabled. Do not grant a folder-wide
exception, turn off Defender, or run ordinary eef/eefn startup as administrator.
Windows may require one legitimate administrator approval to create a persistent
network exception. Application code cannot silently supply that approval.

## Current transport boundaries

eef's authenticated node gateway listens on the configured `node.host` and
`node.port`; the production defaults remain `0.0.0.0` and TCP 51335 so other PCs
can connect. Its owner HTTP interface defaults to localhost, TCP 51334. eefn's
owner HTTP interface defaults to localhost, TCP 51336, and its ordinary connection
to eef is outbound. Do not expose either owner HTTP interface to solve node
connectivity. A configured peer-transfer receiver is a separate, owner-approved
listener; its network requirements are not an invitation to allow all inbound
traffic to the node.

Windows application rules use the executable's full path, not just its filename.
A different temporary test directory or versioned executable path is not covered
by an exception for another copy named `eef.exe` or `eefn.exe`. A conflicting
explicit block rule also takes precedence over an allow rule. See
[Microsoft's rule documentation](https://learn.microsoft.com/en-us/windows/security/operating-system-security/network-security/windows-firewall/rules).
Do not add duplicate allow rules or automatically delete existing block rules.

For a real network installation, any future setup integration must show the
exact executable, required TCP port, network profile and remote-address scope
before applying an exception. Private local-subnet access does not cover an
overlay adapter classified as Public. Do not change that adapter's category or
open all Public-network access automatically. Updates that change the listening
executable path require an explicit rule-lifecycle design; one-time setup is not
yet claimed to cover every future installation path.

## Explicit one-time rule setup

`tools/configure-windows-firewall.ps1` is a development/operator command, not
yet an installer feature. Without `-Apply` it only reads policy and prints a
JSON plan. Supply the exact installed executable and its configured listening
port. Mixed-case role input is normalized to the lowercase project name.
Choose the installation rather than a temporary test copy. For example:

```powershell
# Preview only; replace this example path with the real installation.
./tools/configure-windows-firewall.ps1 -Role eef -Executable 'C:\apps\eef\eef.exe' -LocalPort 51335
```

After reviewing the plan, rerun the same command with `-Apply` from a terminal
you have explicitly opened as administrator. The script does not open an
elevation prompt or install startup elevation. An existing exact managed rule
is reused. A changed executable path or listening port has a distinct identity;
conflicting managed rules are refused, not overwritten. `-Apply -WhatIf` does
not create a rule. Other enabled inbound allow rules for that application also
require review rather than creating duplicates. Old rules are not automatically
removed.

The rule allows **only TCP, that executable and port, the Private profile and
LocalSubnet**. It does not expose the localhost dashboard, allow UDP, change
adapter categories, disable notifications, or change Defender. It conservatively
reports potential application/global block filters and refuses creation when
they exist; the report is not a complete policy-precedence or reachability test.

For `-Role eefn`, select only an explicitly enabled peer receiver's listening
port. An ordinary outbound-only node connection needs no inbound exception.
Do not select the node's owner dashboard port. Public-classified VPNs and
non-local-subnet peers need a separately reviewed rule; this command deliberately
does not broaden its scope for them.

`tools/test-windows-firewall.ps1` uses mocked policy commands and harmless empty
fixture files. It checks preview, stable/distinct identities, exact-rule reuse,
conflicts, block reporting, WhatIf and non-admin refusal without firewall writes.
It is not evidence of elevated creation, prompt suppression, physical networking
or installer/update/uninstall lifecycle acceptance.

## Same-PC regression fixtures

Template-based local fixtures use `tools/local-test-network.mjs`. It changes
only the fixture's node gateway host to `127.0.0.1`, verifies that its owner HTTP
host already uses loopback, and refuses unexpected template layouts. It never
edits the production template, personal configuration, firewall rules or network
profiles. Existing assertions, authentication and approval workflows remain.

`tools/test-local-test-network.mjs` checks newline preservation, refusal cases,
the production LAN default and migrated template readers. CI runs this check.
Deliberately wildcard-bound owner-API rejection tests and the real no-config
first-initialization fixture remain distinct, unchanged gates. The latter creates
new executable paths and a production LAN listener, so it can still require a
Windows firewall decision on an interactive development PC. Do not rerun that
test unattended merely to dismiss or reproduce a prompt.

This fixture isolation reduces unnecessary network-wide test listeners. It is
not evidence of universal Windows prompt suppression, real LAN acceptance,
installer firewall integration, code signing or Defender clearance.
