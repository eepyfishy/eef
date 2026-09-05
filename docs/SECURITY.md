# Security model

- The REST API binds to `127.0.0.1` by default. Add authentication and TLS at a
  trusted reverse proxy before exposing it to another network.
- The node gateway binds to `0.0.0.0:51335` so remote nodes can connect. Every
  frame is AES-256-GCM encrypted and nodes authenticate with HMAC-SHA256.
- `EEF_NODE_PSK` must be a non-placeholder value of at least 12 characters. A
  randomly generated 32-byte value is preferable. Keep it identical on the
  coordinator and nodes, and do not commit it.
- Authentication rejects stale timestamps, replayed nonces, mismatched node
  IDs, unsupported protocol versions, and duplicate live node IDs.
- Filesystem access requires explicit existing roots. Roots and existing path
  ancestors are canonicalized so NTFS junction/symlink escapes are rejected.
- Process execution is not a shell: it accepts only exact existing absolute
  executables from the node allowlist, passes arguments directly, limits
  runtime and output, and kills timed-out children.
- HTTP requests require explicit hosts and methods. Redirects are disabled;
  unless private networking is explicitly enabled, DNS answers are rejected
  if any resolve to a private/non-routable address and the accepted address is
  pinned for the request.
- Filesystem, process, application, HTTP/STT, Wake-on-LAN, microphone,
  audio/TTS, camera, screen, and keyboard/mouse abilities are default-deny.
  Wake-on-LAN additionally requires exact MAC, IPv4 broadcast, and port
  allowlists. Enable the smallest required capability only.
- Python plugins are arbitrary native user code despite running in short-lived
  subprocesses. Treat a plugin as trusted software; isolated mode is not a
  security sandbox.
- Updates verify the artifact SHA-256 from the manifest and use versioned
  directories with rollback. Manifest authenticity still depends on HTTPS and
  control of the manifest host; signing is a future hardening item.
- Firmware source contains Wi-Fi and node secrets. Generated firmware belongs
  in the ignored `data/firmware` directory and should not be published.
- The release executables are not code-signed yet. Windows may warn before the
  first installer launch; verify the release SHA-256 when authenticity matters.
- The installer embeds `requestedExecutionLevel=asInvoker`, installs below the
  current user's local application-data directory, and creates only a per-user
  Startup entry. It neither requests nor bypasses administrator access.
