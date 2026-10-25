# Command-based finite audio relay setup (unreleased)

These commands extend the development tree after `v0.4.0a6`; existing published
installers do not gain them automatically. They configure the running node's
existing backend services. No browser, model, JSON file, resource UUID or port
number is required for the ordinary setup steps. This is not complete non-coder
media acceptance: pairing, Python/audio dependency availability, directional
discovery, reviewed request approval/execution and physical two-PC tests still matter.

## Prepare a role, then give separate permission

On a microphone/source node:

```text
eefn relay configure --microphone
eefn permissions allow peer-transfers
eefn permissions allow microphone
eefn restart
eefn relay show
```

On a speaker/receiving node:

```text
eefn relay configure --receive
eefn permissions allow peer-transfers
eefn permissions allow audio-output
eefn restart
eefn relay show
```

Use `configure --microphone --receive` for both roles on the same node. Setup
creates only missing resource declarations, reuses existing unambiguous bindings
and preserves permissions, credentials, identity, model selections and unrelated
settings. It never opens a microphone, plays sound, loads a model, opens a TCP
listener or restarts. Explicit restart may interrupt other work; it is not a
physical-stop guarantee. Permission commands also require separate owner consent.
Neither setup nor permission consent approves or executes an audio job.

`relay show` explains saved/applied switches and resources, restart requirements,
adapter availability and the current finite-audio advertisement. Advertisement
is a control-session observation, not proof a peer can connect or sound works.
All three permission layers remain required: peer transport, the appropriate
endpoint media switch, and separately reviewed/approved execution grants.

## Receiver address and existing resources

Receiving defaults to the local interface observed on this node's connected,
pinned-TLS coordinator connection, or an existing saved receiver route if there
is no current observation. The observation is cleared when the session retires
and on app runtime replacement. It is not a trusted peer-discovery grant, NAT
traversal, firewall configuration or connectivity test. Loopback is prominently
reported as loopback-only; another PC requires a reachable local LAN/VPN address:

```text
eefn relay configure --receive --address LOCAL_NUMERIC_IP
```

The OS selects a candidate TCP port by binding a temporary socket **without
listening or connecting**. That socket is closed, not kept as a reservation.
An existing route on the same IP keeps its port so repeated setup is stable.
Only an approved consumed receive grant creates the actual listener; a later
collision can still fail that grant without automatic reconnect/replay. Advanced
owners can supply `--port`; no DNS lookup, wildcard, multicast, broadcast or
guessed default port is introduced by setup. To explicitly discard an old route
before selecting another candidate, use `configure --send-only`, then configure
receiving again and explicitly restart. Old uncertain jobs are not resumed.

Existing disabled or incompatible bindings are not silently replaced. Multiple
resources of one needed capability require an explicit selection. Copy the
node's own ID from `relay show` into `--transport-resource`,
`--microphone-resource` or `--speaker-resource`; local and this node's fully
qualified IDs are accepted, another node's are rejected. `--microphone-device`
is an optional existing adapter index; absent preserves the binding or uses the
OS default. Current built-in playback supports only the Windows default output.
Selectors identify the binding this configuration command inspects/edits; they
do not set a default for future jobs or restrict other declared resources. Each
approved audio job still binds its own exact transport and media resource IDs.
Device enumeration, audio-device switching, virtual call devices and mixing are
not implemented by these setup commands.

## Configure from eef

```text
eef node relay --node NODE_ID show
eef node relay --node NODE_ID configure --microphone
eef node relay --node NODE_ID configure --receive
```

Writes require the node owner's current local `remote-management` approval;
relay configuration cannot enable that approval itself. Read-only status and
all writes use the same node-owned implementation as local commands. There is
no generic configuration fallback for old nodes, changed runtimes or uncertain
replies. After uncertainty, inspect `relay show` before deciding what to do;
do not generate automatic retries. The CLI inspects the exact running node and
saved configuration digest itself; users do not supply runtime IDs or hashes.

## Shared backend contract and limits

Local clients use `/api/commands/relay`; the eef adapter uses
`/api/commands/node/relay` and the existing authenticated `node.configure`
control channel. Typed `show`/`configure` requests validate exact node/runtime
identity, a saved relay-configuration digest, roles and bounded selectors.
Writes recheck current local owner consent and merge under the existing config
lock. Reports include resource IDs and capabilities, not private adapter bindings.
No second permissions authority,
job runner, journal, dashboard controller or work-approval path is added.

Finite recording/transfer/playback still uses the existing
[paired grants and content contract](PEER-TRANSPORT.md). Finite requests now have
[typed JSON-free authoring](AUDIO-REQUESTS.md); this is not plain-language
interpretation or automatic work approval. Continuous media, physical resource
quotas/strong stop, presence, virtual devices, public installer/signing/Defender
acceptance and stable v0.4.0 acceptance remain unfinished.
